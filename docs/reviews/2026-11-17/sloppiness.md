# Code-quality review, 2026-11-17: Part II.4: sloppiness

Reviewed at `e6c49e1`; line numbers refer to that commit. Parts: [summary](README.md) · [duplication](duplication.md) · [tests](tests.md) · [comments](comments.md) · [sloppiness](sloppiness.md) · [other](other.md) · [appendices](appendices.md)

## II.4 Sloppiness


### II.4 [S3] arkiv-db-storage, arkiv-db-memory, arkiv-db-redb src

*(repo-relative paths)*

**S1. `crates/arkiv-db-memory/src/persistent.rs` (691 lines: 454 code + 237 tests) is dead.** `PersistentMap`, `Probe`, `Range` are re-exported at `arkiv-db-memory/src/lib.rs:12` and used by nothing:

```
$ rg -n 'arkiv_db_memory' crates conformance | rg -v MemoryVersionedStore
crates/arkiv-db/src/lib.rs:89:pub use arkiv_db_memory::MemoryStore;
$ rg -l 'PersistentMap' --glob '!target' --glob '!vendor'
README.md  docs/architecture.md  docs/node-storage-plan.md
crates/arkiv-db-memory/src/persistent.rs  crates/arkiv-db-memory/src/lib.rs
```

The `Probe` / `Range` / `range_from` the engine uses come from `arkiv_db_trie` (`crates/arkiv-db/src/engine.rs:100`, `typed.rs:27`). `README.md:236` calls it "a reference structure; the engine used it until node step N3b". Fix: delete the file and `lib.rs:5,12`; adjust `README.md:236`, `docs/architecture.md:57,86,189`, `docs/node-storage-plan.md:74`. The code itself is good; git history keeps it. -691 lines. **safe**.

**S2. Unused or needlessly `pub` items.** Each verified with `rg -n '<name>' crates conformance scripts`.

Dead (no use at all):
- `testing/contract.rs:1201-1209` `pub fn objects_of` - definition is the only hit. DELETE (-9, and the `BTreeMap` import at `:15`).
- `testing/faulty.rs:277-284` `FaultyStore::clear_faults` - all five `.clear_faults(` hits are `CrashDisk::clear_faults` on a `disk`. DELETE (-8).
- `testing/faulty.rs:58,64` `ReadScope::Everything` - never constructed. DELETE (-3).
- `testing/faulty.rs:99` `FaultCounters::commits_faulted` - incremented at `:380`, `:403`, never read. DELETE (-4).
- `crates/arkiv-db-redb/src/store.rs:336-339` `RedbStore::config` - `rg '\.config\(\)'` has no hit. DELETE (-4).
- `crates/arkiv-db-redb/src/testing.rs:283-286` `CrashDisk::disarm_crash` - definition only. DELETE (-4).
- `crates/arkiv-db-redb/src/testing.rs:502-507` `CrashBackend::disk` - `rg '\.disk\(\)'` has no hit. DELETE (-7).
- `crates/arkiv-db-redb/src/testing.rs:138-144` `DiskCounters::{reads, writes, set_lens}` - written at `:637, :650, :678`, never read (`rg '\.counters\(\)'` shows only `.syncs` in `tests/durability.rs:48,50` and `.mutating_calls`). They also duplicate `store.probe().stats()`, since `with_backend` wraps every `CrashBackend` in a `CountingBackend` (`store.rs:306`). DELETE the three (-9).

`pub` but only used inside the defining file (make private / `pub(crate)`; all **safe**):
- `versioned.rs:254` `Table::check_record` (callers: `:397`, `:450`, own test).
- `testing/faulty.rs:83` `ReadFault` and its re-export at `testing/mod.rs:16`: no API takes or returns it; `fail_reads` (`:245`) builds it.
- `testing/contract.rs:100`, `:107` `head`, `object` (see the helper inventory).
- `crates/arkiv-db-redb/src/store.rs:56` `object_table_name` (re-exported at `lib.rs:82`, no outside user), `counting.rs:81` `IoProbe::is_killed`, `testing.rs:64` `SECTOR`.
- `crates/arkiv-db-redb/src/testing.rs:228` `durable_image`, `:238` `unsynced` (+ the `pub enum LoggedOp` `:71`), `:374` `is_cut_off`: only called by the unit tests at `:687-853`. `durable_image()` is also exactly `crash_image(CrashMode::DurableOnly)` (`:391-393`).
- `set_latch_corrupt` (`faulty.rs:232`), `clear_read_faults` (`:254`), `injected` / `FaultCounters` (`:287`), `set_latching(false)`: used, but only by the contract's own `faulty_*` cases (`contract.rs:896, 933, 998, 1014, 1043, 1085, 1120, 1122, 1198`). `set_latch_corrupt` in particular is a mode no backend has ("the contract allows a store to latch it; redb does not", `faulty.rs:183`) whose only caller is the 12-line block that tests it (`contract.rs:997-1008`): a feature and its test, with no consumer. Consider dropping both (-25).

**S3. Neither `testing` module is feature-gated.** `crates/arkiv-db-storage/src/lib.rs:16` `pub mod testing;` (1669 lines) and `crates/arkiv-db-redb/src/lib.rs:76` `pub mod testing;` (854 lines) are compiled into every build, including the `arkiv-node` binaries (`crates/arkiv-node/Cargo.toml:16-17` are normal dependencies). No `[features]` section exists anywhere under `crates/`. Users are only `tests/` directories, `crates/arkiv-db-redb/examples/redb_experiment.rs:32`, `conformance/golem-db/src/golem_tests.rs:1155` and redb's own `#[cfg(test)]` module (`store.rs:1244`); `testing/mod.rs:11` says so itself: "Nothing here is used by production code paths."
Fix: `[features] testing = []` in both crates; `#[cfg(any(test, feature = "testing"))] pub mod testing;`; `features = ["testing"]` on the dev-dependencies of arkiv-db-memory, arkiv-db-redb, arkiv-db, arkiv-node; arkiv-db-redb needs the self dev-dependency trick (`arkiv-db-redb = { path = ".", features = ["testing"] }`) for its own `tests/` and `required-features = ["testing"]` on the `redb_experiment` example; the conformance harness must enable it. Two intra-doc links then need the feature (`crates/arkiv-db-storage/src/versioned.rs:35-36`, `crates/arkiv-db-storage/src/lib.rs:12`). **needs-care** (build wiring only).

**S5. Long function / big modules.**
- `stage_relabel`, `crates/arkiv-db-redb/src/store.rs:654-740` (87 lines): one meta block plus three near-identical "count must match, then remove each and compare" blocks (`:699-714`, `:715-723`, `:724-735`). Split: `retire_meta(&mut meta, retired)` and one `retire(&mut table, name, expected, remove: impl FnMut(..) -> Result<bool, Fault>)`. The messages are also inconsistent: `format!("{} record count", table.name())` at `:706` but the literals `"inclusions record count"` / `"block_index record count"` at `:717`, `:726`, which are what `table.name()` returns. Clippy: `.map(|v| v.value() == value) != Some(true)` -> `.is_none_or(|v| v.value() != value)` at `:710, :720, :732`.
- `crates/arkiv-db-redb/src/store.rs` (1309 lines) -> `store/mod.rs` (types, `RedbStore`, trait impls), `store/stage.rs` (`stage`, `stage_relabel`, `shape`, `insert_only`: `:552-766`, 215 lines), `store/open.rs` (`Opener`, `path_opener`, `create_file`, `names_file`, `sync_parent_directory`, `open_database`, `prepare`, `check_meta`: `:768-1015`, 248 lines), `store/report.rs` (`OpenReport`, `LayoutStats`, `TableLayout`, `DataStats`: `:99-163`).
- `crates/arkiv-db-storage/src/versioned.rs` (1298 lines) -> the stats half (`SpaceStats`..`StatsRecorder`, `:933-1142`, 210 lines) is self-contained and moves to `versioned/stats.rs`; the error type and its constructors (`:462-604`) to `versioned/error.rs`.
- `testing/contract.rs` (1209 lines): base cases `:179-861`, faulty cases `:863-1199`; split along that line when the macro list is split (T6).

**S4. Magic array indices for timings and buckets.**
- `crates/arkiv-db-redb/src/store.rs:212` `timings: [OperationTiming; 5]`, indexed by literal at `:1028` (`[0]`), `:1094` (`[1]`), `:1069` (`[2]`), `:1138` and `:1156` (`[3]`), `:1174` (`[4]`), with the names in a parallel array at `:354`, and the test at `:1281-1285` asserting on `timings[0]`..`[4]`.
- `crates/arkiv-db-redb/src/counting.rs:32` `timings: [OperationTiming; 4]`, literals at `:135` (`[0]`), `:165` (`[1]`), `:156` (`[2]`), `:147` (`[3]`), names at `:57`.
- `crates/arkiv-db-redb/src/metrics.rs:9` `[u64; 12]`, `:37` `[u64; 12]`, `:45` `[AtomicU64; 13]`, `:63` `self.buckets[12]`, `:127` `cumulative_buckets[11]`.

Fix: `#[derive(Clone, Copy)] #[repr(usize)] enum StoreOp { ReadObject, ReadRecord, Head, Commit, Recover }` with `const ALL` and `const fn name(self)`, and `self.timings[StoreOp::Commit as usize]`; same for the four backend ops; `const BUCKETS: usize = DURATION_BUCKETS_NANOSECONDS.len();` and `[AtomicU64; BUCKETS + 1]`. **safe**; the snapshot arrays that reach the node's metrics page keep their order and length.

**S6. Needless clones / allocations.**
- `objects.to_vec()` in both `stage_objects` (D5).
- `testing/contract.rs`: clippy `redundant clone` at `:407` (`Some(bytes.clone())` -> `Some(bytes)`), `:504` (`next.clone()` -> `next`), `:766` (`bytes.clone()` -> `bytes`), `:958` (`Some(bytes.clone())` -> `Some(bytes)`).
- `crates/arkiv-db-memory/src/versioned.rs:34` keys records by `(Table, Vec<u8>)`, so every lookup allocates a key: `key.to_vec()` at `:116`, `key.clone()` at `:228`, `:264`, `:282`. `records: [BTreeMap<Vec<u8>, Vec<u8>>; 5]` indexed by `table as usize` (or a `BTreeMap<Table, BTreeMap<..>>`) looks up by `&[u8]`. It is the reference store used by most node tests. **safe**.
- `versioned.rs:516` `out.push_str(&format!("{byte:02x}"))` allocates a `String` per byte; `write!(out, "{byte:02x}")` with `fmt::Write` (clippy `format_push_string`).
- `crates/arkiv-db-redb/src/store.rs:1031` (and `:573`, `:453`): `object_table_name(space)` formats a `String` on every object read. Small next to a redb read transaction; mention only.

**S7. Overly defensive / inconsistent.**
- `testing/faulty.rs:116-117` `recover_failures: u32` + `recover_kind: Option<StorageErrorKind>`, read with `unwrap_or(StorageErrorKind::Io)` at `:428`. `fail_next_recoveries` (`:266-270`) always sets both, so the `None` arm cannot happen; store `recover_fault: Option<(u32, StorageErrorKind)>` or a plain `StorageErrorKind`.
- `testing/faulty.rs:400-411`: the `AckLost` arm bumps `commits_faulted` and formats `"injected commit fault: acknowledgement lost"` by hand; the `injected` closure at `:378-382` exists for exactly that (`injected(StorageErrorKind::Io, "acknowledgement lost")`).
- `testing/faulty.rs:62-71` `ReadScope::covers`: four arms returning `true` (clippy `match_same_arms`); one `matches!` with an `|` pattern plus the `Space` guard.
- `versioned.rs:73` `|| bytes == [0u8; 16]` after a successful `/dev/urandom` read guards a 2^-128 event.
- `versioned.rs:170` `"expected 40"` hard-codes `Head::ENCODED_LEN` (`:154`); `<[u8; Self::ENCODED_LEN]>::try_from(bytes)` would also replace the two manual `copy_from_slice` blocks (`:173-176`). Bytes unchanged; `[CONSENSUS]` only in the sense that this is the on-disk head form - do not change the layout.
- `crates/arkiv-db-redb/src/store.rs`: `self.slot.read().unwrap_or_else(PoisonError::into_inner)` is spelled out at `:383-386`, `:427`, `:510` although a `lock()` helper exists for the mutexes (`:229`).
- `crates/arkiv-db-redb/src/store.rs:1166-1171` `health`: `self.failure().map_or(Health::Available, Health::Failed)`.

### II.4 [D2] Scripts, CI, build config, vendor/reth-trie, conformance

*(repo-relative paths)*

#### S1. bench-compare.sh ships one machine's paths, including a temporary session directory - behaviour-affecting (defaults)
- scripts/bench-compare.sh:28 `CONF_LOCAL_SEED="${BENCH_CONF_SEED:-<temporary session directory>/conf/repo}"`.
  The default is a temporary session directory on one machine; `rg` finds that path only in this file. This is the
  single most visible "written by an assistant" artifact in the chunk. Fix: default to empty and skip the seeding branches
  (321-325, 343-346) when unset.
- :24 `REPO="${BENCH_REPO:-/home/ubuntu/arkiv-network/arkiv-state-db-experiment}"` although `SCRIPT_REPO` is computed
  from the script's own location one line earlier (:23). Fix: `REPO="${BENCH_REPO:-$SCRIPT_REPO}"`.
- :25 `WORK="${BENCH_WORK:-/home/ubuntu/arkiv-bench}"`. Fix: `${BENCH_WORK:-${XDG_CACHE_HOME:-$HOME/.cache}/arkiv-bench}` or
  require it.
- :260 `local envfile="/home/ubuntu/arkiv-network/.env"`, from which `git_with_token` reads `GITHUB_TOKEN`. No secret is
  in the repo, but the script reaches for a credential file at a fixed absolute path. Fix: `${BENCH_GITHUB_ENV:-}` and
  return 1 when unset.
- :36 `OLD_REV="e230fb3"` is a fixed baseline commit as default; fine if documented, otherwise make `--old-rev` required.
docs/benchmarking.md:25-26, 41, 50, 138 repeat the two `/home/ubuntu` paths and need the same edit.

#### S2. Scripts that are obsolete by their own description are still shipped
- **S2a. scripts/sdk-compat-smoke.py + .ts (307 + 455 = 762 lines)** - DELETE candidate, safe. Their headers (py:4-7,
  ts:4-7) and docs/sdk-compatibility.md:3-9 say they target the payload-era SDK 0.8.x and are "expected to fail" against
  current nodes; udc-sdk-smoke.{py,ts} replaces them. Still referenced as runnable at docs/sdk-compatibility.md:240 and 295;
  those two snippets go with them. (sdk-compat-smoke.ts:450 also writes a `transferHeight` nobody reads.)
- **S2b. the codec-2 prefill pipeline (2,446 lines)** - needs-care, owner decision. scripts/sourcify-prefill.py:784 writes
  `ARKPFL02` with row codec 2; the node accepts only codec 3 (crates/arkiv-node/src/genesis_bundle.rs:20-21
  `ROW_CODEC: u32 = 3`, checked at genesis_streaming.rs:120-121), and docs/sourcify-prefill-converter.md:3-12 says
  "Outdated by user-defined columns ... which current nodes refuse". The runner is wired only to it:
  run-full-prefill.py:176-178 calls `<converter> convert --bundle --out --reserve-gib [--expiry-policy ...]` with no
  `--chain-id`, which sourcify-v4-prefill.py:918-919 refuses, and `--expiry-policy` exists only in the old tool;
  full-prefill-bootstrap.py:119 hard-codes `sourcify-prefill.py`. So sourcify-prefill.py (972), test-sourcify-prefill.py
  (738), run-full-prefill.py (267), test-run-full-prefill.py (278), full-prefill-bootstrap.py (125) and
  test-full-prefill-bootstrap.py (66) can produce nothing a current node starts from. Either port run-full-prefill.py to
  the v4 converter (add `--chain-id`, drop the expiry flags) and delete the old converter and its test (1,710 lines), or
  delete the whole pipeline. Docs that reference it: docs/sourcify-prefill-converter.md, full-prefill-docker.md,
  streaming-genesis-v2.md, streaming-prefill-100k-validation.md, genesis-adoption.md, and the comments in
  docs/examples/{far-future,unlimited}-prefill/chain.toml.
- **S2c. scripts/full-prefill-bootstrap.py cannot run from this repository** - it loads `SNAPSHOT/'runtime.py'` (:95) and
  calls `runtime.s3_location`, `.client`, `.BoundedReader`, `.extract`, `.verify_bundle`;
  `rg -n 'def s3_location|def verify_bundle|class BoundedReader|runtime\.py' --glob '!target' .` finds only that line. It is
  the bootstrap of one October job (docs/full-prefill-docker.md:55-68). Archive it with that run's notes or add the module.

#### S3. Dockerfile.simulator does not build what exists, and nothing references it - behaviour-affecting
Dockerfile.simulator:3-5 copies `Cargo.toml`, `Cargo.lock`, `crates`, `vendor`, then :6 builds `-p arkiv-node --bin arkiv-simulator`.
The binary name and path are right (crates/arkiv-node/src/bin/arkiv-simulator.rs). But the arkiv-node *library*
includes a file from `docs/`: crates/arkiv-node/src/genesis_authoring.rs:48-49
`const PRESET: &str = include_str!("../../../docs/examples/genesis-bundle/profiles/fixed-glm-v2.toml");` (module is
unconditional: lib.rs:34 `pub mod genesis_authoring;`). `docs/` is not copied, so the build stage should fail at that
`include_str!` (by reading; not built here). Nothing would notice: `rg -n -i 'dockerfile|docker build' --glob '!target' --glob '!vendor' .`
= 0 hits, and no workflow builds the image. Fix, smallest first: add
`COPY docs/examples/genesis-bundle/profiles ./docs/examples/genesis-bundle/profiles` before the build (safe), or move the
preset next to its sibling `src/genesis_presets/arkiv-entity-v3-glm-v2-4.toml` and point the docs and the two scripts
that read it (udc-sdk-smoke.py:77, full-indexer-smoke.py:132) at the new path (needs-care; the bytes must not change
`[CONSENSUS]`). Then either reference the Dockerfile from docs/simulator-node.md and build it in CI, or delete it. Also:
`EXPOSE 9400` (:17) but the binary's default listener is `127.0.0.1:9400` (arkiv-simulator.rs:46 usage text), so a
container needs `--listen 0.0.0.0:9400`; say so in a comment.

#### S4. vendor/reth-trie is a workspace member that nothing uses - safe
`rg -n 'reth_trie|reth-trie|KeyHasher|ForwardInMemoryCursor' crates conformance scripts` = 0 hits; no crate manifest has
`reth-trie` (`rg -n 'reth-trie' crates/*/Cargo.toml` = 0); Cargo.lock lists it with no dependents. Yet it is a member
(Cargo.toml:9), a workspace dependency (Cargo.toml:26), in CI's fmt/clippy package list (.github/workflows/ci.yml:22), in
the README crate table (README.md:241) and both README commands (README.md:260-261). 243 lines of Rust plus two licence
files compiled, formatted, linted and tested for no consumer. Delete the directory and those five references, plus
docs/engine-storage-v1-report.md:220, 222, docs/signed-proposer-v1.md:272 and the "Reth extraction" section of
vendor/README.md:8-25 and :51-52. If it is being kept for a planned use, say so in vendor/README.md.

#### S5. Cargo.toml hygiene
- crates/arkiv-node/Cargo.toml:34 `alloy-trie.workspace = true` (dev-dependency) is unused:
  `rg -n 'alloy_trie' crates/arkiv-node` = 0 hits. DELETE. safe
- crates/arkiv-node/Cargo.toml:32-33 lists `alloy-primitives` again under `[dev-dependencies]` with the comment
  "Independent Keccak-256 for the golden digests of tests/block_format.rs"; it is already a normal dependency at :10, so
  the entry does nothing. DELETE both lines. safe
- Exact `=` pins without a reason at crates/arkiv-node/Cargo.toml:12 (`alloy-sol-types = "=1.7.3"`), :13
  (`k256 = "=0.13.4"`), :18 (`ed25519-dalek = "=2.2.0"`), :21 (`toml = "=0.8.23"`), next to caret requirements for
  everything else (:19-28). The root manifest shows the right pattern: the redb pin has a one-line reason (Cargo.toml:30).
  With `Cargo.lock` committed and `--locked` in CI the pins add nothing unless they are deliberate (signature crates
  pinned for determinism would be a fair reason): add the reason or relax to caret. needs-care
- No duplicated declarations to hoist: every third-party crate of arkiv-node has a single consumer; the shared ones
  (alloy-primitives, alloy-rlp, alloy-trie, redb) are already workspace dependencies.
- `[workspace.lints.rust] unsafe_code = "forbid"` only (Cargo.toml:32-33): thin but honest. If anything is added, add what
  this review keeps finding by hand: `[workspace.lints.clippy] dbg_macro = "warn"`, `todo = "warn"`, and try
  `rust.unused_crate_dependencies` once (it would have flagged the alloy-trie entry above).

#### S6. Unreferenced files (candidates for deletion or for one line in a doc)
Method: for every tracked file under scripts/, `rg -l --fixed-strings <basename> README.md CHANGELOG.md AGENTS.md docs .github scripts Dockerfile.simulator deploy conformance crates`.
- **scripts/query-order-bench.py** (262 lines): no reference anywhere. Its docstring says it reproduces "the 2026-10-22
  external node benchmark", and no document under docs/ records that run.
- **Dockerfile.simulator**: no reference (S3).
- Referenced only by a single doc, each as a manual step: arkiv-v2-demo.py (docs/arkiv-v2-compatibility.md:74),
  glm-accounts-smoke.py, range-proof-smoke.py, seed-bricks.py, sourcify-v4-node-check.py, sdk-side-by-side-bench.ts,
  full-indexer-smoke.py. Fine, but none is exercised by CI, so they rot silently (S2 is what that looks like later).

#### S7. scripts/full-prefill-bootstrap.py and its test are written in code-golf style - safe
One-space indentation, statements joined with `;`, no spaces around operators, comma-joined imports
(full-prefill-bootstrap.py:1), lines up to 370 characters (:9, :77, :95, :115, :117, :119), `ROOT`/`SNAPSHOT`/`PLAN`/`STATUS`
as upper-case locals inside `main` (:86-87), `except BaseException` wrapping everything (:120), a subclass defined inline
to monkeypatch `runtime.BoundedReader` (:105-109), and a hard-coded sponsor address in the `execv` argv (:119).
test-full-prefill-bootstrap.py (66 lines) is in the same style (e.g. :32, :36). These two files look unlike everything
else in scripts/. Run a formatter over both and split `main` into `load_credentials`, `acquire`, `extract_and_verify`,
`exec_runner`. The download logic itself (15-82) is careful and its nine tests are real; only the form is the problem.

#### S8. genesis-bundle-smoke.py writes a report into a directory it then deletes - safe
:499 `(D / "report.json").write_text(...)` writes into the `TemporaryDirectory` opened at :143, which is removed when the
`with` block ends 12 lines later. So `reports["identity"]` (:418), `["genesisQuery"]` (:420), `["firstLiveCreate"]` (:451),
`["expiry"]` (:482, :487), `["passed"]` (:498) and the write are dead, and the other `reports[...]` entries are just
locals with a longer name. Delete them, or add `--work-dir` like streaming-prefill-smoke.py:62 and keep the report.
Also :145-146 `wallets = WALLETS; expected = EXPECTED` are aliases with no purpose; `WALLETS` (98-102) restates the three
addresses already in `FIXTURE` (:43, :48, :53) and in `EXPECTED` (:107, :117, :127); `B` (:24) and `D` (:144) should be
`BINARY` and `work` like the other scripts.

#### S9. Smaller dead or defensive code
- udc-sdk-smoke.py:243 and sdk-compat-smoke.py:276 `producer[-1] = "paused"`: the list already ends in `"paused"`
  (udc-sdk-smoke.py:188, sdk-compat-smoke.py:183). No-op left from an earlier `"running"`. DELETE. safe
- run-full-prefill.py:165 `getattr(a, 'genesis_install', 'copy')`: argparse always sets the attribute (:224). The getattr
  exists only because test-run-full-prefill.py:111-114 builds a `Namespace` without it. Pass `genesis_install='copy'` in
  the test and use `a.genesis_install`. needs-care (test edit). `handoff` is also reused for two different lists (:179, :211).
- sourcify-v4-node-check.py:51 uses `urllib.error.HTTPError` but the file imports only `urllib.request` (:25). It works
  because `urllib.request` imports `urllib.error` internally; add the import. :65 `def bundle(dir)` shadows the builtin.
- scripts/bench/parse_run.py:141-142 `env = ....split(); rec["env"] = [e for e in env if e]`: `str.split()` never yields
  empty strings; the comprehension is a no-op.
- seed-bricks.py:32 `namespace.request = request` monkeypatches the module it just loaded so that `find_namespace` picks up
  the retrying wrapper. Give `find_namespace` a `request=` parameter (create-sample-namespace.py:22) instead.
- conformance/golem-db/ci.sh:70: `status` is sticky across the loop, so after a failure with retention `''` the second
  iteration prints `::error::... (ARKIV_STATE_RETENTION='0')` even when retention 0 passed. Use a per-iteration variable
  and OR it into `status`. behaviour-affecting (log text only)
- conformance/golem-db/src/arkiv.rs:863, 872 `check_name(&self, ..)` / `check_value(&self, ..)` do not use `self`: make
  them free functions like `to_value` (1084). :343-345 `prepare_bytes: 1 << 30, prepare_objects: 4_000_000,
  prepare_visits: 100_000_000` are bare numbers 30 to 100 times the engine defaults (crates/arkiv-db/src/durable/mod.rs:55-57);
  one comment saying why (the 2 MiB-bytes scenarios?) would do.

#### S10. Inconsistent defaults across sibling scripts - behaviour-affecting if unified
- Default binary: `target/debug/arkiv-proposer` relative to the repo root in the smokes (signed-proposer-smoke.py:15,
  genesis-bundle-smoke.py:24, udc-sdk-smoke.py:27, streaming-prefill-smoke.py:63) but `target/release/...` in
  full-indexer-smoke.py:42; and relative to the *current directory* in create-sample-namespace.py:41
  (`'target/debug/arkiv-proposer'`) vs seed-bricks.py:95 (`'target/release/arkiv-proposer'`): two scripts meant to be run
  one after the other default to different builds.
- Where the indexer checkout is expected: `ROOT.parent / "arkiv-chain-indexer"` (sdk-compat-smoke.py:36) vs
  `ROOT.parent.parent / 'arkiv-network' / 'arkiv-chain-indexer'` (full-indexer-smoke.py:40). Both encode one person's
  directory layout; both are overridable through `ARKIV_INDEXER_DIR`.
- Fixed default ports: arkiv-v2-demo.py:24 (`--base-port 19400`), full-light-demo.py:246 (`--base-port 8781`). Both are
  interactive demos with a flag, so acceptable; all smokes use ephemeral ports.

#### S11. Three formatting styles, no formatter configured - safe
Black-style explosion (genesis-bundle-smoke.py, udc-sdk-smoke.py, sdk-compat-smoke.py: 89 / 46 / 52 lines that hold one
short argv token each, e.g. genesis-bundle-smoke.py:289-303 and 362-414), hand-wrapped at 120-200 columns
(signed-proposer-smoke.py, glm-accounts-smoke.py, range-proof-smoke.py, sourcify-*.py), and golfed (S7). Quotes are double
in most files and single in streaming-prefill-smoke.py, full-indexer-smoke.py, create-sample-namespace.py, seed-bricks.py,
run-full-prefill.py and its test. The two `.ts` smokes differ the same way: tabs and semicolons (sdk-compat-smoke.ts) vs
two spaces and none (udc-sdk-smoke.ts). Lines over 150 characters
(`awk 'length > 150 {print FILENAME":"FNR": "length}' scripts/*.py scripts/*.ts scripts/*.sh`):
sourcify-prefill.py 10 (max 254), test-full-light-demo.py 9 (max 236, e.g. :75 a 236-character boolean), full-prefill-bootstrap.py 6
(max 370), test-full-prefill-bootstrap.py 4, full-light-demo.py 4 (:221, :227, :309, :321; separately :187 hides a walrus inside a conditional inside a lambda),
arkiv-v2-demo.py 4 (max 225), test-sourcify-prefill.py 3 (max 387 at :106), seed-bricks.py 3, range-proof-smoke.py 3,
glm-accounts-smoke.py 2, bench-compare.sh 2, signed-proposer-smoke.py 1, full-indexer-smoke.py 1 (:204, a 302-character
SQL string). One `ruff format` config (line length about 110) and one commit would end this.

#### S12. Functions with an obvious split
- scripts/test-full-light-demo.py:328-433 `test_run` is 105 lines with six phases, each ending in its own `print("PASS: ...")`
  (372, 376, 387, 407, 416, 430). Split into `sync_and_restart`, `batch_recovery`, `oracle_queries`, `proof_tampering`,
  `bad_header`, `capacity_and_identity`, each taking `(cluster, proxy, history)`. :405 joins two statements with `;`.
- scripts/test-full-light-demo.py:63-159 `Proxy.Handler.do_POST` is 97 lines that mix routing, three mutation families and
  response writing: extract `mutate_sync(result, method, mode)` (110-128) and `mutate_proof(data, mode)` (133-151).
- scripts/sourcify-v4-prefill.py:621 `scan_line` returns a 10-tuple, which scan_part patches by position at :665
  (`summary[:4] + (...,) + summary[5:]`) and `Converter.row` unpacks at :835. A `NamedTuple` (`Row`) with `_replace(refs=...)`
  removes the positional coupling. Same file: `if kind in ("str",):` (:358) is `kind == "str"`; one-letter names `m`
  (:789, :927), `c` (:955), `t` (:894) for the manifest, the converter and the totals.
- scripts/sourcify-prefill.py:154 `import stat` inside `open_relative` (the v4 copy imports it at the top, :81).
  scripts/test-sourcify-prefill.py:518 `import gzip` inside a test; :315-316 a doubled blank line inside the class.

#### S13. Hard-coded `True` flags in reports
streaming-prefill-smoke.py:298-300 writes `'fullDownloadedPrefill': True, 'lightHasNoPrefill': True,
'verifiedAccountProof': True, 'verifiedEntityProof': True, 'liveWrite': True, 'restart': True,
'verifiedEntityProofAfterRestart': True` into `acceptance.json`; sourcify-v4-node-check.py:110 `"inCreationOrder": True`
and :125 `"matchesReport": True`. They are literals, true only because the lines above did not raise. A reader of the
JSON takes them for measurements. Replace with one `"checks": [...]` list of the names, or drop them. safe

### II.4 [T3] arkiv-db tests: queries / records / pagination / terms

*(paths relative to `crates/arkiv-db/tests/`; `qm` = `query_metering.rs`)*

#### 4.1 Call-site boilerplate that hides the assertions in `pagination.rs` - safe (~150 lines)

42 multi-line `.query(ns, &query, &projection, 7, Some(&cursor), Budget::unlimited())` calls (8 lines each); 8 in `cursor_from_another_snapshot_or_a_branch_is_a_mismatch` (1439, 137 lines), 7 in `cursor_bound_to_another_namespace_query_or_projection_is_a_mismatch` (1648, 154 lines), 6 each in `cursor_check_comes_after_..` (1804) and `a_cursor_after_any_record_..` (2324). `branch_cursor_*` shows the shape that works: `second_page(&branch, cost_ns, &cursor)` (1927-1936). Give the other cursor tests the same one-liner over the shared view trait (1.4):

```rust
fn continue_with(view: &impl View, ns: NamespaceId, query: &Query, projection: &Projection, cursor: &Cursor) -> ReadResult<QueryPage>
```

and each `assert_mismatch(..)` becomes 1-2 lines; the two long tests drop to ~60 lines each without losing a case.

Also `pagination.rs:1155-1159` / `1313-1317`: the common ledger prefix is kept in an array and spliced by index, `seek[0], seek[1], seek[2],` (5 times) and `open[0], open[1], open[2],` (3 times). Use `[&seek[..], &[(PostingEntry, 3, 1, 3), (ResultRow, 2, 3, 6)]].concat()` or a `fn eq_ledger(entries: u64, rows: u64) -> Vec<..>`.

#### 4.2 Long test functions with an obvious split

Clippy `too many lines` has 18 hits in these files (17 tests and `Head::disturb`); the ones worth splitting:

| fn | lines | split |
|---|---|---|
| `queries.rs:1614` `query_faults_are_reported_in_the_validation_order_before_any_charge` | 263 | `earlier_faults_mask_later_ones` (1629-1682) / `every_literal_fault_alone_and_negated` + `values_at_the_limit_are_accepted` (1683-1773) / cursor block 1774-1828 deleted (2.7) / `validation_precedes_the_directory_lookup_and_ignores_the_budget` (1830-1875) |
| `records.rs:1579` `argument_faults_are_admission_rejections_with_nothing_recorded` | 210 | move the 18-row table to `fn argument_fault_cases(ns) -> Vec<(&'static str, WriteOp, ArgumentFault)>`; split off `values_and_cell_counts_at_the_limit_are_admitted` (1777-1787) |
| `queries.rs:1290` `patches_and_deletes_move_records_between_indexes` | 192 | `..patches..` (1299-1372) / `..deletes_and_recreation..` (1374-1465) / `the_published_state_answers_the_same` (1467-1480) sharing a `mutated_world()` builder |
| `records.rs:2265` `raw_bytes_and_typed_records_share_a_namespace_but_never_a_key` | 156 | at "A record delete leaves raw entries alone" (2372) |
| `pagination.rs:1136`, `1439`, `1648` | 156, 137, 154 | shrink via 2.6 / 4.1 rather than split |
| `terms.rs:1082` `terms_validation_faults_..` | 155 | the two tables (`chain`, `single`) are 4 lines per row after rustfmt; a small `fn case(tag, lower, limit, fault)` or a type alias for the row brings each row to one line (-45 lines) |
| `terms.rs:1239` `terms_validation_precedes_the_directory_lookup` | 110 | five 9-line `view.terms_read(unknown, &price, TypeTag::I64, Bound::Unbounded, 1, <budget>)` calls: a closure `let probe = |tag, limit, budget| view.terms_read(unknown, &price, tag, Bound::Unbounded, limit, budget);` halves it |

#### 4.3 `queries.rs:2187-2210` `page(..)`: 7 positional arguments, 4-tuple result, 22 call sites - safe (~110 lines)

Every call is 9 lines after rustfmt and most discard two of the four results (`let (ids, _, receipt, _) = page(&snapshot, ns, &query, &Projection::ID_ONLY, 10, None, unlimited);`). 13 of the 22 calls pass `Projection::ID_ONLY`, no cursor and a plain budget; 16 of the 31 destructurings discard two of the four results. Two tests already work around it with a local closure (`run` at 2347-2357 and 2403-2413). Fix, if the section survives 2.2:

```rust
struct Costs { snapshot: Snapshot, ns: NamespaceId }
struct Got { ids: Vec<u64>, after: Option<u64>, receipt: Receipt, next: Option<Cursor> }
impl Costs {
    fn ids(&self, query: &Query, limit: usize) -> Got;                    // ID_ONLY, no cursor, unlimited
    fn page(&self, query: &Query, projection: &Projection, limit: usize, cursor: Option<&Cursor>, budget: Budget) -> Got;
}
```

The six `let unlimited = Budget::unlimited();` aliases (2217, 2346, 2439, 2529, 2568, 2639) go with it.

#### 4.4 Vacuous or self-referential assertions - safe

- `queries.rs:1252-1264`: wrong cell name makes the check vacuous (2.15).
- `records.rs:675`: `assert_eq!(rid(7), seven);` with `let seven = rid(7);`.
- `terms.rs:2271`: `assert_eq!(EXPIRY_TERMS_READ, 16)` against its own definition (2.8).
- `records.rs:733`: `let indexable = ..filter(|t| !t.is_indexable())`: the binding name says the opposite of its contents (2.16).

#### 4.5 Leftover code - safe

- `queries.rs:1775-1779`: `let bogus = query(&Query::match_all(), &Projection::ID_ONLY, 1).expect("one row, one more to come").value.next; assert_eq!(bogus, None, ..)`. A first attempt at getting a cursor from the one-record fixture that could not work; the real cursor comes from `foreign` (1780-1807). DELETE 5 lines (or the whole block, 2.7).
- `queries.rs:902`: `#[allow(clippy::too_many_arguments)]` on `Reader::page_with`, which has 7 parameters including `&self`. Clippy's default threshold is 7 and fires above it; there is no `clippy.toml`; the pedantic run has no `too many arguments` hit anywhere, including the free fn `page` at 2187 with 7 parameters and no allow. DELETE the attribute.
- `records.rs:441, 462-464, 485-487`: `TypedView::raw_count` is called only on a `Branch` (2348, 2362, 2375, 2383, 2419); the `Snapshot` impl is dead and the method is a rename of `Branch::record_count`, which the same file calls directly at 929 and 2182. DELETE the trait method, call `record_count`. -9 lines.
- `records.rs:2672` `let (engine, ns, _model) = fixture_abc();`, `2674-2675` `retype` and `twenty` are the same value (`vec![(name("price"), attr(Value::U64(20)))]`) under two names, `2687` `drop(probe)` on a value that is not used again and holds no lock. Use one binding, drop the `drop`. (`terms.rs:1067` `drop(scratch)` is the same; there it at least reads as "discarded".)
- `pagination.rs:2202` `let mut at_head = model.clone();` (clippy `redundant clone`: `model` is not used afterwards); `records.rs:1715`, `1737`, `terms.rs:690`, `1156` likewise (last use of the cloned value).

#### 4.6 Charge-table constants defined but not used where they matter - safe

- `pagination.rs:100-106` names the seven weights, then every ledger line in the file uses bare numbers: `(OperationClass::DirectoryLookup, 1, 1, 1)`, `(OperationClass::QueryGroup, 1, 5, 5)` (1156-1158, 1176-1177, 1195-1196, 1314-1316, 2061-2065, ...). The names are referenced only by the `const _` asserts.
- `queries.rs` has no weight names at all: 30 ledger tuples with literal unit prices and hand-multiplied subtotals (2248-2253, 2369-2374, 2460-2465, 2500-2505, 2591-2596).
- `records.rs`/`terms.rs` do it properly: 3-tuple lines `(class, count, UNIT)` with the subtotal computed (`records.rs:223-235`).

Fix: one `support::fixture_prices` module with the weights these files use and the 3-tuple `ledger()`; drop the fourth tuple field in `queries.rs`/`pagination.rs` (61 tuples; `rg -c '\(OperationClass::[A-Za-z]+, \d+, \d+, \d+\)'`). Mechanical; **needs-care** only because the ledgers are pinned vectors: the computed subtotal must equal the literal it replaces (it does: each is `count * unit`).

#### 4.7 Placement and scoping - safe

- `records.rs:2423-2429` `read_admission` is defined 860 lines after its first use (1561) and 2180 lines after its sibling `admission` (245). Move next to `admission`. Same for `entry` (2257).
- `records.rs:1586` `use ArgumentFault::{..};` after six `let` statements; `records.rs:2277-2294` eleven `const`s declared between statements inside the test (clippy `items after statements`, 17 hits). Hoist to the top of the function.
- `terms.rs:1832-1843`: builds `Vec<(String, Cells)>`, then a second `Vec<(&str, Cells)>` with a clone of every cell list, only because `populate` takes `&[(&str, Cells)]`. Make `populate` take `&[(impl AsRef<str>, Cells)]`.
- `terms.rs`: `sweep(..).unwrap_or_else(|halt| panic!("an unlimited sweep completes: {halt}"))` seven times (2052, 2090, 2105, 2124, 2127, 2135, 2256). Add `fn sweep_all(branch, ns, block) -> Vec<(RecordId, Receipt)>`.
- `queries.rs:1609-1611` `fn lit(term) -> Literal { Literal::of(term) }`: a 3-line alias called 18 times next to 9 direct `Literal::of(` and 8 `Literal::not(` in the same file. Pick one spelling.
- `arkiv_db::NamespaceName::new("test").unwrap()` with the full path, 10 times here (`queries.rs:622, 1569, 2122`; `records.rs:290, 1471, 2146`; `terms.rs:236, 1964`; `pagination.rs:627, 2037`), 66 times in 13 test files; `pagination.rs:2044` `arkiv_db::ArgumentFault::NotHostOperation` likewise. Reads like a field added later and patched in by search-and-replace. Import the type and use one `create_namespace(branch, actor) -> NamespaceId` helper (`pagination.rs:622-635` has it).

#### 4.8 `terms.rs:295-308` `value_bytes`: 24 variants to reimplement `Value::encoded_len()` for four types - safe

Clippy flags the duplicated arms (297/298, 303/304: "identical bodies"). The model only ever holds Bool, I64, U64 and Str (`INDEXABLE`, line 80; every fixture). Either call `value.encoded_len()` (pinned by `records.rs:774-783`) or keep the independent oracle honest and small:

```rust
fn value_bytes(value: &Value) -> u64 {
    match value {
        Value::Bool(_) => 1,
        Value::I64(_) | Value::U64(_) => 8,
        Value::Str(text) => u64::try_from(text.len()).expect("lengths fit"),
        other => panic!("model: {other:?} is not used by this file"),
    }
}
```

Related: `terms.rs:498-499` `TypeTag::Bytes => panic!(..), _ => vec![]` in `probes`: a tag outside the four would silently get only the `Unbounded` probe. Make it `_ => unreachable!(..)`. `terms.rs:1596-1597`: two `unreachable!` arms where one wildcard does.

#### 4.9 Three ways to check a constant derivation (see 2.8) - safe

Comment only (`records.rs`), run-time `assert_eq!` on consts (`terms.rs:2035-2049, 2063-2066, 2162-2173, 2216-2227`), compile-time `const _` (`pagination.rs:112-197`).

#### 4.10 Named constants next to the same magic numbers - safe

`pagination.rs`: `WALK_RECORDS = 64` (79) but `assert_eq!(model.len(), 64)` (687) and `Some(64)` (1049); `BOOKS = 50` (80) but `50` at 2196 and 2259; `MAX_PAGE = 200` (72) is a second copy of `Limits::DEFAULT.max_page`, checked at run time (649) instead of `const MAX_PAGE: usize = Limits::DEFAULT.max_page;` (the literal 200 is pinned by `records.rs:844`).

### II.4 [N3] arkiv-node binaries, genesis_*, query_language

*(paths relative to `crates/arkiv-node/src/`)*

1. **`Result<_, String>` in seven genesis files; the binary prints and discards.**
   - `type Result<T> = std::result::Result<T, String>` plus a local `err`: genesis_bundle.rs:29/36,
     genesis_authoring.rs:29/274, genesis_streaming.rs:22/23, genesis_bulk.rs:27/30 (+ `debug_err` :33),
     genesis_prebuilt.rs:33/34, genesis_adopt.rs:36/37, genesis_import_resources.rs:5/6. Five different prefixes,
     `{e:?}` in three and `{e}` in four, about 300 string error sites.
   - bin/arkiv-proposer.rs:186-189 `authoring_error(String) -> Error` does `eprintln!` and returns
     `Error("InvalidGenesisBundle")`; 24 uses. The real message and the exit line (`arkiv-proposer:
     InvalidGenesisBundle`) are two unrelated stderr lines, and a library caller that is not the CLI
     (signed/host.rs:200-201 `map_err(|_| Error("InvalidPrefillBundle"))`) loses the cause entirely.
   - Cleanest fix, in two steps:
     a. One `pub struct GenesisError(String)` in a `genesis::error` module with `Display` (the message, unchanged),
        `std::error::Error`, `From<&str>`, `From<String>`, and one
        `GenesisError::context(stage: &str, e: impl Display)` that produces today's `"{stage}: {e}"` text for
        wrapped foreign errors (literal messages carry no prefix today, e.g. the exact
        `"prefill stream changed during import"` asserted at genesis_streaming.rs:1039). One alias
        `genesis::Result<T>`. The seven aliases and seven `err` functions go; `"..".into()` and
        `.ok_or("..")?` keep compiling through the `From` impls. **needs-care**: mechanical in the library;
        the ~45 test assertions need `.to_string()` (or `impl PartialEq<&str>` plus a `contains` passthrough).
     b. In the proposer make `run()` return `Result<(), CliError>` with
        `enum CliError { Node(Error), Genesis(GenesisError), Usage(String) }` and `From` impls. All 23
        `.map_err(authoring_error)` become `?`, `authoring_error` is deleted, `main` prints one line with the
        cause. **needs-care**: tests/prebuilt_genesis_cli.rs:312 and :445 assert stderr contains the cause and
        `InvalidRequest`; both still hold.
     Turn the newtype into an enum with kinds (`Io`, `Mismatch`, `Limit`, `Busy`) only when a caller needs
     to branch; today nothing branches on a genesis error, the callers only print it.

2. **bin/arkiv-proposer.rs `run()` is 679 lines (:604-1282), cognitive complexity 43.** Split along the
   `if command == ..` chain that is already there:
   - `keygen(&Args)` :676-683
   - `sign_command(&Args, kind)` :684-763, with `transfer_tx` :694-709 and `create_tx` :711-757 returning a `TxEnvelope`
   - `submit(&Args)` :764-806
   - `read_limits(&Args) -> ReadLimits` :833-852
   - `open_producer(..) -> Host` :888-1071, itself `resume_producer` :909-944 and `init_producer` :945-1050
   - `open_follower(..) -> Node` :1072-1189, with `pinned_genesis` :1089-1112, `fetch_small_prefill` :1133-1151
   - `serve(rt, ..)` :1190-1282 with `production_loop` :1215-1266
   - Parse the role once into `enum Role { Producer, Full, Light }`: `command` is compared as a string 18 times
     (`rg -c 'command == "|command != "|command.as_str\(\)'`), and :1200-1204 maps it back to a string.
   - Per-subcommand flag lists then replace the flat `FLAGS` (Other 2). **needs-care**, no behaviour change intended.
   - Also: the closure `store` (:870) shadows the function `store` (:64) and calls it; rename to
     `open_store` / `tracked_store`. `local_prefill` :223-236 early-returns on `prefill.is_none()` and then does
     `.as_ref().ok_or(Error("InvalidPrefillBundle"))?` on the same option: bind it once with `let Some(launch) = .. else`.
     `create_host` :326-338 matches `other` again inside the catch-all arm: write the `Small` and `None` arms out.

3. **Help text as one string literal.** bin/arkiv-proposer.rs:615 is 3,633 characters on one line, :618 is 538,
   bin/arkiv-simulator.rs:46 is 1,155. rustfmt skips the enclosing statements. The genesis usage is repeated at
   bin/arkiv-proposer.rs:592 (299 chars), which also leaves :588-593 unformatted. Fix: `const USAGE: &str =
   concat!("line\n", "line\n", ..)` with one source line per output line (bytes identical, **safe**), a
   `const GENESIS_USAGE` shared by :592 and the help, and the ~2.5 KB of operational prose (retention,
   read limits, adoption) either wrapped the same way or moved to docs/ with a pointer.
   Not in the help: `--batch-bytes` (proposer), roles `inspect` / `inspect-light` (simulator :74, documented
   only in docs/simulator-node.md:133).

4. **Deprecated `--batch-bytes` and the parameter chain behind it.** bin/arkiv-proposer.rs:518, 527-532, 543 ->
   `DEFAULT_BATCH_BYTES` genesis_streaming.rs:33 -> `build_bundle` :444, `build_bundle_with_state` :462,
   `bulk_build` :345, `bulk_build_with` :368 -> `genesis_bulk::build_file(.., ingestion_bytes, ..)` :125.
   There it does two things: a range check (:129-131, "batch bytes must be 1..=4194304") and gating
   `progress.report` (:300-303, `report_at` :216), which `Progress::report` already throttles to one line per
   10 s (genesis_streaming.rs:61-67; `audit_file` :257 calls it per row). Fix: delete the parameter from five
   signatures, the constant, the range check and `report_at`; call `progress.report` per row. Keep
   "accepted and ignored" in the CLI (5 lines) if compatibility matters; docs/streaming-genesis-v2.md:286,
   docs/sourcify-prefill-converter.md:172, docs/streaming-prefill-100k-validation.md:232 mention the flag.
   ~25 lines plus one test loop. **behaviour-affecting** only for the progress cadence and the flag.

5. **Unused or over-visible items.**
   - `StreamReader::prefix_hash` genesis_streaming.rs:144-146: `rg -n prefix_hash crates conformance scripts docs`
     -> the definition only. DELETE. **safe**.
   - `genesis_adopt::STAGED_STATE` (:42), `genesis_prebuilt::MANIFEST` (:46), `genesis_bundle::RECIPE_DOMAIN` (:16):
     `pub`, used only in their own file (`rg -c` shows no other file). Make private.
   - `pub type AuthoringReport = serde_json::Value` genesis_authoring.rs:28: an alias used by three signatures
     in the same file; nothing else names it. Inline or keep, low value.
   - `StreamReader` has `pub rows`, `pub consumed`, `pub bytes` (genesis_streaming.rs:102-104): mutable public
     fields on a validating reader; readers only (genesis_bulk.rs:210-302, genesis_streaming.rs:254-257). Make
     them private with getters.

6. **Five `#[allow(clippy::too_many_arguments)]`.** bin/arkiv-proposer.rs:275 (`create_host`, 9 args) and :344
   (`adopt_host`, 10 args) share seven: bundle them in `struct Launch { state, history, genesis, source,
   certificate, signer, options }`. genesis_bulk.rs:117 (`build_file`, 9), genesis_streaming.rs:336, 360: after
   items 4 and Duplication 3 `build_file` has 8; `struct Import<'a> { base, limits, sponsor, summary, permanent }`
   brings it to 4.

7. **genesis_bulk.rs `build_file` is 251 lines (:118-368).** Split: `claim_scratch(work_dir, identity) ->
   (File, PathBuf)` :144-169 (lock + IDENTITY); `ingest(reader, &mut builder, ..) -> (units, burn)` :207-313;
   `credit_sponsor(&mut builder, ..)` :314-330; `seal(pending, ready, ..)` :331-367. Inside the row loop:
   - :228-238 builds an 8-field `ArkivExecutionContext` per row only to call `arkiv::expiry` (:250), which
     reads `c.block_height` alone (arkiv/mod.rs:341-353). Hoist the context out of the loop, or give
     `arkiv::expiry` a `block_height: u64` parameter. `[CONSENSUS]`-adjacent, **needs-care**.
   - :243-257 moves `row.attributes` into an `ArkivOperation::Create` to call `op.validate()`, then gets them
     back with `let .. = &op else { unreachable!() }`.
   - :252-254 repeats a check the reader already made with the same flag (genesis_streaming.rs:212-217) and
     that `prefill_rent_wei` makes again at :267 (genesis_bundle.rs:444-449). One is enough.

8. **Two more long functions with obvious splits.** genesis_authoring.rs `compile` :834-1028 (195 lines):
   `load_profile` :863-878, `load_allocations` :879-919, `load_rows` :955-992, `normalize_rows` :993-1005.
   genesis_streaming.rs `build_bundle_with_state` :456-585 (130 lines): `read_base` :470-487,
   `publish_bundle(stage, ..)` :536-575. bin/arkiv-simulator.rs `run` :38-307 (270 lines): `inspect` :117-158,
   `open_producer` :160-212, `open_follower` :213-257, `serve` :258-306. bin/arkiv-demo.rs `main` :33-167 (135).

9. **Same constant names, different values, in two modules.** `STREAM_DOMAIN`, `MAX_ENTITIES`,
   `MAX_CANONICAL_BYTES`, `MAX_STORAGE_UNITS` exist in genesis_bundle.rs:12-15 (256, 1 MiB, 8 MiB, `…/v1`) and
   genesis_streaming.rs:26-32 (100,915,200, 1 TiB, 2^50, `…/v2`). Callers must qualify every use and tests mix
   both (genesis_streaming.rs:694-698). Rename the small ones `SMALL_*` when the modules move. The stream
   magics `b"ARKPFL01"` (genesis_bundle.rs:229, 251) and `b"ARKPFL02"` (genesis_streaming.rs:120) have no
   constant at all.

10. **Work done twice.**
    - genesis_authoring.rs:1007-1009: encode the stream, decode it (which re-encodes to check canonicity), then
      `build_genesis_with`, which encodes again and whose `construct` decodes again. `canonical` is only the
      sorted rows and `build_genesis_with` sorts anyway (genesis_bundle.rs:176-177, 605). Pass `&entities` and
      drop :1008; better, let `build_genesis_with` return the stream it encoded. **safe** `[CONSENSUS]` (same bytes).
    - genesis_bundle.rs:633 decodes the whole stream to compare a row count; `construct` (:637) decodes it again
      at :473. Compare `built.rows.len()`.
    - `construct` builds a JSON report per row (:559) for every caller, including `verified_candidate` and
      `build_genesis_with`, which drop it. Return the plain numbers and format in `report`.
    - genesis_bundle.rs:153-155 and :186 both test `row.attributes.len() > MAX_ROW_ATTRIBUTES`; the second can
      never be true. Remove it and "or excessive" from the message at :193.

11. **Formatting rustfmt gave up on.** Lines over 150 characters:
    bin/arkiv-proposer.rs:592 (299), :615 (3633), :618 (538); bin/arkiv-simulator.rs:46 (1155), :139 (261),
    :143 (321), :154 (347); genesis_bundle.rs:559 (246), :659 (583); genesis_streaming.rs:597 (466);
    genesis_authoring.rs:1666 (174, a test fixture string); simulator/query_language.rs:156 (171), :158 (180).
    The `json!` one-liners (simulator :139, 143, 154; genesis_bundle.rs:559, 659; genesis_streaming.rs:597) format
    normally once each key is on its own line. bin/arkiv-simulator.rs:153-155 builds a second `json!` object,
    clones its map and `extend`s the first: write the six keys with `report["k"] = ..` or build one object.
    Item boundaries with no blank line: genesis_authoring.rs 48, genesis_streaming.rs 47, genesis_bundle.rs 32,
    bin/arkiv-proposer.rs 21 (every item from :31 to :604), genesis_prebuilt.rs 17.

12. **`use` inside bodies.** genesis_bundle.rs:558 (`use crate::simulator::wire::hex;` inside the row loop) and
    again :650. Move to the file's imports.

13. **`Progress::report(rows, bytes, checkpoint, complete)`** genesis_streaming.rs:53-59: `checkpoint` is `None`
    in seven of eight calls (genesis_streaming.rs:255, 257, 260; genesis_bulk.rs:210, 301, 313, 335) and
    `Some(0)` once (genesis_bulk.rs:347). Nothing reads `"checkpointSequence"`
    (`rg checkpointSequence crates scripts docs` -> the emitting line only). Drop the parameter and the field.
    **needs-care** (log shape).

14. **Two ways of writing the same stderr event stream.** Library: `writeln!(std::io::stderr().lock(), ..)` at
    genesis_streaming.rs:77 and genesis_bulk.rs:205. Binary: `eprintln!("{}", serde_json::json!(..))` at
    bin/arkiv-proposer.rs:391-395, 412-416, 934-937, 1007-1010. Fix: one `pub fn event(value: serde_json::Value)`
    and, in the library, take it as a sink so unit tests that run a bulk build stop printing progress JSON.

15. **Fully-qualified paths instead of imports in bin/arkiv-proposer.rs.** `arkiv_node::genesis_streaming::` 18
    times (`verify_stream_file` 4, `check_disk` 4, `prepare_verified_streaming` 2, …), `arkiv_node::genesis_bundle::`
    6 (`MAX_CANONICAL_BYTES` 4), `arkiv_node::genesis_authoring::` 4, `arkiv_node::signed::metrics::` 3. Add
    `use arkiv_node::{genesis_authoring, genesis_bundle, genesis_streaming::{self, VerifiedStream, check_disk,
    verify_stream_file}}`. **safe**.

16. **Clippy items checked.**
    - genesis_bulk.rs:97 "suspicious operators … did you mean `complete.units > limits.units`": false positive.
      The receipt's units are bounded by the launch limit; `StreamSummary` has no units field. Leave.
    - genesis_authoring.rs:1293 unused `self`, redundant clones genesis_streaming.rs:865, 1044, 1103,
      genesis_prebuilt.rs:753: real, see section 2.
    - genesis_streaming.rs:597 "`format!` to build up a string from an iterator": real, use `wire::hex` (Duplication 5).
    - bin/arkiv-proposer.rs:1059 stray `;` after the `match`.

17. **Port artifacts in the local parts of query_language.** `use std::string::{String, ToString}; use
    std::vec::Vec;` (lexer.rs:25-26, error.rs:8, parse.rs:27-29, literal.rs:19), `use std::vec;` (lexer.rs:474),
    `std::format!` written out 24 times (literal.rs 18, parse.rs 6): the README says crate imports were
    replaced with `std`, and the swap left prelude items imported by hand. literal.rs:19-28 interleaves `use`, `const` and
    `type`. Either accept as the cost of staying diffable, or clean once and record it in README.md.

18. **simulator/query_language.rs `classify`.** :160-165: `eligible` is `matches!(tree, Query::Eq { .. })`, then
    the match arm `Query::Eq { key, value } if eligible` repeats it, and `let reason = explanation;` renames a
    binding. Write `let selection = match &tree { Query::Eq { key, value } => Some(selection(key, value)),
    _ => None }; let eligible = selection.is_some();`. `Language.status`, `Capability.status`,
    `Classification.shape`, `Diagnostic.code` are `String`s holding one of a few literals: serde enums with
    `rename_all` give the same JSON. **needs-care** `[CONSENSUS]` (HTTP body).
    `selection` :192-198 is the inverse of `builtin_from_name` (parse.rs:490-495): put `BuiltIn::name()` in ast.rs.

### II.4 [N2] arkiv-node runtimes: signed / simulator / demo

*(paths relative to `crates/arkiv-node/`)*

1. **Code rustfmt gave up on** (the strongest "generated" tell in the chunk). Lines > 150 chars: signed/runtime.rs
   16 (426=662, 588, 757, 758, 762=280, 763, 765, 766=455, 768, 770, 780=343, 856=439, 924=725, 925=408,
   926=433, 927=409); signed/host.rs 4 (656=376, 695=475, 700, 705); simulator/runtime.rs 4 (135=267, 150=868,
   492=283, 724); simulator/light.rs 2 (413, 439=565); simulator/host.rs 1 (492); demo/runtime.rs 3 (417, 630, 716=464).
   Unformatted regions (no spaces around operators, several statements per line):
   signed/runtime.rs:756-783 (whole body of `http_post`) and 917-929 (verify closure of `verified`);
   simulator/runtime.rs:488-493; simulator/inspection.rs:58-109 and 131-190 (22 lines > 100);
   simulator/light.rs:411-418; demo/runtime.rs:630-636.
   Blank lines between items: signed/runtime.rs 3 in 1236 lines, signed/host.rs 6/1228, simulator/runtime.rs 4/916,
   simulator/host.rs 3/739, simulator/light.rs 1/456, demo/runtime.rs 5/965, demo/workload.rs 0/161.
   Fix: (a) write every `json!({..})` with one key per line (rustfmt will then format the surrounding code);
   (b) split `http_post` into one function per endpoint (see 5); (c) one blank line between items.
   **safe**, adds lines. Reordering keys while reformatting cannot change a response body: `serde_json` is built
   without `preserve_order`, so `Value` objects serialise key-sorted.

2. **Stringly-typed node state.** `health: &'static str` (simulator/host.rs:283, simulator/light.rs:94,
   signed/host.rs:133, signed/light.rs:24), `health: String` (demo/runtime.rs:53), `role: &'static str`
   (simulator/host.rs:285), all `pub`, all compared by literal: simulator/host.rs:442, 472;
   simulator/light.rs:200, 232; simulator/runtime.rs:877, 909; signed/host.rs:715-716, 829, 867, 890, 911, 1097;
   signed/light.rs:144-146; signed/runtime.rs:975; demo/runtime.rs:803, 823, 855-856 (the demo compares against the
   `Debug` spelling of `DemoError` stored at 807/826). 76 lines with such literals in the chunk, 10 more in the two
   binaries (`rg -c '"(storage-fenced|chain-conflict|following|stalled|paused|running|syncing)"' src`).
   Same for `status == "applied"` at simulator/metadata.rs:212, 263 (a string the function assigned at 175-195) and
   the control `action: String` matched twice (simulator/host.rs:120-126 then 469-505; signed/runtime.rs:966-982,
   with 974 re-testing `c.action == "pause"` inside the `"pause" | "resume"` arm).
   Fix: `enum Health { Paused, Running, Following, Stalled, ChainConflict, StorageFenced }` with
   `as_str()`/`Serialize` giving today's strings, `enum Role { Producer, Full }`, `#[derive(Deserialize)] enum Action`.
   **needs-care**; JSON spelling **[CONSENSUS]**.

3. **Consequences of `Error(&'static str)` in these files** (70 distinct codes, 121 `map_err(|_| Error(..))`).
   - Dead arm: simulator/runtime.rs:338 `"CoverageUnavailable"` - never constructed
     (`rg '"CoverageUnavailable"' crates conformance scripts docs` -> that line only). DELETE. **safe**.
   - Unreachable arm: simulator/runtime.rs:335 `"UnknownRun"` - constructed only at `bin/arkiv-simulator.rs:89, 118`
     (CLI start-up of the inspect roles), never on a path that reaches `response_with`. DELETE. **safe**.
   - Hand-written round-trip tables: simulator/runtime.rs:581-598 (15 arms of `Some("X") => "X"`), signed/runtime.rs:1079-1087.
     Anything not listed collapses: a peer's 400 `InvalidRequest` becomes a local 503 `StorageUnavailable` (simulator 597).
   - Error detail smuggled around the type: `HistoryPruned` cannot carry `(requested, floor)`, so the host keeps a
     `Refusal(Cell<Option<(u64, u64)>>)` (simulator/host.rs:290, signed/host.rs:142) and both runtimes re-lock the node
     after the request to fetch it (`refusal`: simulator/runtime.rs:307-315, signed/runtime.rs:441-449). The re-lock is a
     `try_lock().ok()?`, so the detail is dropped when the lock is busy, and the cell is per host, not per request:
     with 4 admission slots a second refused request can overwrite or take the first one's detail.
     **behaviour-affecting** to fix (carry the detail in the error value).
   - An error code passed as a parameter: `proof_state(&self, snapshot, mismatch: &'static str)` signed/host.rs:1189-1200
     (callers 1173, 1180, signed/runtime.rs:804).
   - Five spellings of "wrong node role": `"UnsupportedNodeRole"` (signed/runtime.rs:547, 564, 571, 583, 593, 802,
     819, 868, 960), `"FullNodeRequired"` (510, 627), `"ProducerOnly"` (778), `"UnsupportedProof"`
     (simulator/runtime.rs:294, 445, 679, 712), `"InvalidRequest"` (simulator/runtime.rs:490, 781, 788, 844, 875;
     signed/runtime.rs:1135, 1177). **[CONSENSUS]** to unify.
   - `"StorageUnavailable"` as the catch-all 503: reqwest client build failure (simulator/runtime.rs:61,
     signed/runtime.rs:135), peer transport failure (simulator 566, 572; signed 1062, 1069 - although signed has
     `"PeerUnavailable"` at 1086), fetch timeout (signed 1037), a missing prefill file (signed 648).
   - Useful causes discarded: signed/host.rs:200-201 (`verified_candidate` error -> `"InvalidPrefillBundle"`),
     signed/protocol.rs:377, 481 (`TxFault`/`BlockFault` -> `"InvalidTransaction"`/`"InvalidInputs"`),
     simulator/host.rs:625-628 and signed/host.rs:987-990 (`ExecError` -> `"InvalidRequest"`/`"InvalidBlock"`).
     signed/host.rs:435-446 admits it ("The node error code alone would hide why a store was refused") and works
     around it with an `eprintln!`.
   - demo/wire.rs:26-28: an odd-length hex string is reported as `ProofLimitExceeded`; simulator/wire.rs:17 then
     flattens every `unhex` failure, including over-cap, to `"InvalidRequest"`.
   The first generation (`DemoError`, demo/mod.rs:9-29: a 19-variant enum that serialises and round-trips through
   `RpcResponse`) was the better design; simulator/mod.rs:46-59 is a 1:1 enum-to-same-spelling table that an enum
   with `as_str()` makes unnecessary.

4. **The signed genesis is re-validated and re-hashed on every use** (signed/protocol.rs).
   `hash()` (240) = `header().hash()`, and `header()` (230-239) encodes and digests the whole `SignedGenesis` each call
   (and calls `self.genesis.header()` twice). `spec()` (226-229) = `validate()` - which builds and drops a `ChainSpec`
   at 221 - plus a second `ChainSpec::new(self.genesis.clone())`.
   `SignedHeader::authenticate` (534-548) calls `genesis.validate()` and `genesis.hash()` each time;
   `SignedTx::validate` (344-386) calls `genesis.spec()` + `genesis.hash()` and encodes `self` at 363 and 380 (again
   inside `hash()`); `SignedInputs::validate` gets `spec` at 458 and each `tx.validate(genesis)` at 476 recomputes it.
   Redundant direct calls: `Decode for SignedGenesis` already validates (282-284), yet signed/host.rs:498-500 and
   signed/runtime.rs:322-323 decode and immediately `validate()` again; `Pool::with_limits` validates at pool.rs:132
   right after `Host::create*` did (host.rs:197/294). 18 `genesis.hash()` call sites and 8 `.spec()` call sites in the chunk.
   A submitted transaction is validated (signature recovery included) at signed/runtime.rs:776 outside the lock, then
   again at signed/host.rs:720 under it, and again at block production (host.rs:919).
   The two caches documented at signed/host.rs:146-157 exist to hide this cost ("re-authenticating a stored record
   (signature recovery, genesis validation) would hold it for milliseconds").
   Fix: validate once into a wrapper that owns the results, e.g. `struct Chain { genesis: SignedGenesis, spec: ChainSpec,
   hash: B256, header: Header }` built by `SignedGenesis::validate(self) -> Result<Chain>`; `authenticate`, `validate`,
   `Pool::new`, `Host` take `&Chain`. Drop host.rs:720 or runtime.rs:776. **needs-care** (many signatures; no encoding change).

5. **Long functions and their splits.**
   - simulator/metadata.rs:71-355 `capture` (275 lines, cognitive complexity 31). Split: `admin_operations` (84-105),
     `expiry_operations` (106-126), `starting_create_nonces` (127-157), `user_operations` (158-300) with
     `fn op_kind(&TxOp) -> &'static str` and `fn op_key(&TxOp) -> Option<String>` replacing the match at 233-262 that
     both yields `kind` and assigns `key` as a side effect, and `terminal_changes` (301-354); carry
     `namespaces`/`records`/`raw` in a small `Touched` struct. **safe**, feed bytes **[CONSENSUS]**.
   - signed/host.rs:906-1095 `execute_publish` (181). Split: `check_nonces` (920-933), `execute` (934-990; the two
     calls at 962-986 take the same nine arguments - select the function, call once), `retain_included` (992-1011),
     `inclusions` (1060-1077), then publish. **safe**.
   - signed/runtime.rs:739-786 `http_post` (dense; ~130 lines once formatted): `text_query` (760-767; the simulator
     already has it as a function at simulator/runtime.rs:614-673), `submit_transaction` (773-779), `resolve_query` (780).
   - signed/runtime.rs:494-603 `read` (108): `headers_or_feed` (513-561), `account_routes` (568-590).
   - signed/runtime.rs:859-930 `verified`: the closure at 917-929 is `page_json(..)`.
   - simulator/runtime.rs:732-863 `sync_once` (131): `check_batch` (759-777), `observe_full` (785-809), `import_body` (811-861).
   - demo/runtime.rs:480-590 `handle` (109): move the `Headers`, `Blocks`, `QueryEq` arms into methods like
     `resolve`/`query_local` already are.
   - demo/workload.rs:51 and simulator/workload.rs:30 `inputs` (109 each) are fixture tables: leave.

6. **`pub` fields used as the configuration API.** Callers build a host and then poke it:
   `host.prefill_file = ..` (`bin/arkiv-proposer.rs:407, 425`, `tests/genesis_adopt.rs:239`, `tests/signed_http.rs:1568`),
   `host.prefill_stream = ..` (`bin/arkiv-proposer.rs:424`), `host.paused = false; host.health = "running"` /
   `"following"` (`bin/arkiv-proposer.rs:1055-1056, 1185-1186`, `bin/arkiv-simulator.rs:209-210`),
   `h.role = "full"; h.paused = false; h.health = "following"` (`bin/arkiv-simulator.rs:237-239, 251-253`;
   `tests/simulator_http.rs:264-265` sets role and paused but not health), and - through an `Arc` -
   `Arc::get_mut(&mut rt).ok_or(..)?.storage_path = ..` (`bin/arkiv-simulator.rs:259-261`),
   `Arc::get_mut(..).unwrap().{storage_path, admin_token, replication_token} = ..` (`tests/simulator_http.rs:108, 129, 272`) because
   `simulator::Runtime::new` reads `SIMULATOR_ADMIN_TOKEN`, `SIMULATOR_REPLICATION_TOKEN` and
   `SIMULATOR_ALLOW_PRIVATE_HTTP` from the environment itself (simulator/runtime.rs:50, 67-68).
   Fix: `Host::start(Role)` (sets paused/health/role together), `Host::open(.., prefill: Prefill)`,
   `struct RuntimeConfig { peer, replication_peer, admin_token, replication_token, allow_private_http, storage_path }`
   with `RuntimeConfig::from_env()` called by the binary; then make the fields private. **needs-care**.

7. **`too_many_arguments` constructors in signed/host.rs** - a config struct is the obvious fix.
   `create` (165-185) is a pure forwarder; its `#[allow]` at 165 is dead (7 parameters; clippy fires above 7 and
   `clippy.toml` sets no threshold). `create_with_prefill` (187, 8), `create_from_staged` (283, 9) and `adopt_staged`
   (376, 8) share `store, history, genesis, source, certificate, signer, options`.
   Fix: `pub struct Launch { store, history, genesis, source, certificate, signer, options }` and
   `pub enum Prefill<'a> { None, Stream(Vec<u8>), Staged { engine: &'a DurableEngine, file: PathBuf } }`;
   `Host::create(launch, prefill)` replaces three functions and all four allows, on top of Duplication 1.
   25 call sites of the four functions: `bin/arkiv-proposer.rs` 3, test modules of `signed/rpc.rs`, `rpc_reads.rs`,
   `rpc_ws.rs` 4, `tests/` 18 (`rg "Host::(create|create_with_prefill|create_from_staged|adopt_staged)\("` in files
   that use `signed::`). **needs-care**.

8. **Hand-rolled router inside a wildcard handler.** `"/sim/v1/{*path}"` and `"/signed/v1/{*path}"` are routed by
   `if path == ".."` chains: simulator/runtime.rs:238-304, 397-466; signed/runtime.rs:494-603, 739-786, 937-993
   (`path.strip_prefix("replication/blocks/")`, `path.split('/')` at 568). axum's router does this; the per-branch
   parse/permit/respond boilerplate is the result. Fix: one `.route()` per endpoint, handlers returning
   `Result<Json, Error>` with `impl IntoResponse for Error`. **behaviour-affecting [CONSENSUS]**: an unknown path
   today answers a JSON `InvalidRequest` (400/422), not axum's 404.

9. **Dead, unused or test-only items** (each verified with `rg` over `crates conformance scripts`):
   - `simulator::wire::ResolveRequest` wire.rs:347-356 - no use anywhere. DELETE (10 lines). **safe**.
   - `demo::workload::DEFAULT_SEED` workload.rs:13 - no use; `bin/arkiv-demo.rs:120` hard-codes `"7"`. DELETE or use it. **safe**.
   - `demo::runtime::rows_json` (629) and `sync_once` (850) are `pub` with no use outside the file. Make private. **safe**.
   - `simulator::inspection::tag_name` (114-116) = `tag.name()`, used only in that file; `typed_value` (111-113) =
     `crate::value_json::to_json`, one outside use (`signed/state_api.rs:92`); `simulator::metadata::value_type`
     (25-27) = `v.tag().name()`, used only at metadata.rs:48, 54. Inline all three (9 lines). **safe**.
   - `simulator::runtime::fetch` (523-531): only caller is `tests/simulator_http.rs:300`; `fetch_with_policy` (532-541)
     has two in-file callers (689, 750) and one in the binary. Three layers (`fetch` -> `fetch_with_policy` -> `fetch_authorized`) for one
     function with two optional arguments. Likewise `validate_peer` (75-77) is `validate_peer_with_policy(peer, false)`.
   - simulator/light.rs:357-374 `verify`/`inspect`: two 9-line wrappers over `verify_page(.., bool)`; the only `src`
     caller (simulator/runtime.rs:714-718) holds the bool and branches to choose the wrapper. Expose one method. ~14 lines.
   - `SignedTx::from_bytes`, `SignedBlock::from_bytes` (signed/protocol.rs:391-396, 611-616): test-only (section 2).
   - `Pool::sender_count` (pool.rs:167): tests only. `PoolLimits`/`Pool::with_limits` (pool.rs:77-94, 131): tests only
     - acceptable as a test seam, but the "may tighten, never expand" validation is unreachable in production.

10. **Needless clones and repeated work.**
    - signed/host.rs:844-845: `self.block(height)?` twice; each call deep-clones the cached `SignedBlock` (595) to compare
      one field. Use `let known = self.shared_block(height)?;`. **safe**.
    - signed/host.rs:861: `block.inputs.clone()` of an owned block; pass the block and move its inputs. **safe**.
    - simulator/host.rs:539-577 `import(record: StoredBlock)`: takes ownership, then clones inputs, workload and
      controls (571-575) and passes `Some(&record)` so `execute_publish` can re-encode it (646). Compute
      `record.bytes()` once, destructure, pass `Option<&[u8]>`. **safe**.
    - simulator/light.rs:265: `self.run.validate()?` inside the per-header loop of `append` rebuilds a `ChainSpec`
      (a genesis clone plus a freshly built demo genesis to compare with, simulator/host.rs:46-57) for each of up to 64
      headers; `create`/`open` (101, 158) call `validate()` and throw the spec away. Store `spec: ChainSpec` in `Light`. **safe**.
    - simulator/runtime.rs:150: `capabilities(&crate::demo::workload::genesis(0, 0).limits)` builds a whole genesis per
      status request to read limits that `RunConfig::validate` (simulator/host.rs:53) guarantees equal
      `run.genesis.limits`. Use that (as signed/runtime.rs:367 does). **safe**.
    - signed/pool.rs: `Dropped` is keyed by hex `String` (32-34); `reason()` lowercases its input per lookup (65-68);
      `by_hash()` (170-176) hex-encodes the hash of every pooled transaction to compare with a `&str`. Key by `B256`,
      parse the caller's string once. `check()` runs twice per admission (via `admissible` from host.rs:736, then
      `insert_funded` 244), and `tx.hash()`/`encode(tx)` are recomputed at 325, 351, 384, 386, 58. **needs-care**.
    - Whole-response re-serialisation per appended item as a size check: demo/runtime.rs:545-552 (up to 16 blocks),
      signed/runtime.rs:549-557 (up to 64 feeds). Track a running size instead. **safe**.
    - demo/runtime.rs:213 and 293 clone the whole `RequestIndex` per submit/block as a rollback mechanism; 316
      re-encodes each dequeued transaction to learn the size computed at 212.

11. **Clippy items the task asked about.**
    - signed/pool.rs:383 "unnecessarily wrapped by Result" - correct. `insert_checked` only returns
      `Ok(Admission::Inserted)`. Make it return `()` and write `self.insert_checked(tx, rent); Ok(Admission::Inserted)`
      at 247. Also 378-380 `self.arrival.checked_add(1).ok_or(Error("ArrivalExhausted"))?;` discards its value: write
      `if self.arrival == u64::MAX { return Err(..) }`. **safe**.
    - signed/runtime.rs:221 "collection is never read" - false positive in effect, awkward in shape. `waiting` is an
      `Option<Count>` filled lazily at 245 and kept only for its `Drop` (decrements `waiting_reads`/`waiting_rpc`).
      Clearer: try once before the loop, then `let _waiting = Count::new(mine);` and loop. **needs-care** (lock
      fairness; `reads_waiting()`/`requests_waiting()` are observed by tests).
    - demo/auth.rs:93 "suspicious operators" - false positive. `self.header.chain_id != trust.genesis.chain_id` is
      right; `TrustConfig` (auth.rs:8-12) has no `header` field, so clippy's suggestion would not compile. No change.
    - demo/runtime.rs:371-375: `resolve` takes five `String`s by value and only borrows them. Take `&str`. **safe**.
    - simulator/host.rs:539: see 10. signed/light.rs:32: `certificate: SignedHeader` by value, only borrowed.
    - demo/workload.rs:107: arms `3` and `9` are identical (`create("recreated", ..)`): `3 | 9 =>`. 109 and 134 repeat
      `actor = if round.is_multiple_of(2) { ALICE } else { BOB }`.

12. **Leftover debug output in library code**: signed/host.rs:442-446 `eprintln!` of a JSON event from
    `Host::adopt_staged`. It is the only `println!/eprintln!/dbg!` in the chunk. Return the cause in the error (see 3)
    and let `bin/arkiv-proposer.rs` (which already prints `prefill_genesis_adopt` events, 412-416) log it. **needs-care**.

13. **Magic numbers** (name them; **safe**): signed/runtime.rs `512` (324, 1122, 1127 - certificate cap), `65536` (322),
    `4096` (794; simulator/runtime.rs:443), `Semaphore::new(4)` (149, while `wire::READ_CONCURRENCY = 4` is in scope),
    retry `10`/`9`/`50 ms` (1026-1029); `65536 + 256` at simulator/runtime.rs:183 and simulator/light.rs:154;
    `156` (encoded `Header` length) at simulator/wire.rs:103, simulator/light.rs:184, demo/auth.rs:65 - use
    `Header::MIN_ENCODED_LEN`; `164` at signed/light.rs:104; `8 * 1024 * 1024 - 116` at signed/host.rs:1057 (the
    simulator names the same bound `ENVELOPE_CAP`, simulator/host.rs:187); manifest entry tags `0/1/2/3` in
    signed/history.rs (67, 74, 116, 117, 122, 161, 294, 389) and control-key prefix `2` / length `40` in
    simulator/host.rs (428, 433, 669); `namespace.ordinal() == 1` at signed/light.rs:420, 456 and
    signed/runtime.rs:924 where simulator/light.rs:58, 410 write `== crate::arkiv::entity_namespace()`;
    `131072` at simulator/workload.rs:144; limits `16, 128, 1024, 128 * 1024, 64, 8, 8` at signed/protocol.rs:190-198
    mirroring demo/workload.rs:21-38 and demo/runtime.rs:78-80; proof cap `RESPONSE_CAP / 2` at signed/runtime.rs:914
    vs `proof_limits().max_proof_bytes` at simulator/runtime.rs:702 for the same purpose.

14. **Module size and layout.**
    - signed/runtime.rs (1236) declares its children with `#[path = "range_api.rs"] mod ..` x3 (lines 2-7) to keep them
      flat. Make it `signed/runtime/` with `mod.rs` (Runtime, lock discipline 77-307), `http.rs` (status, read,
      handlers 308-1011), `sync.rs` (fetch, sync_once 1012-1188) and the three existing children.
    - signed/host.rs (1228): `host/record.rs` (HeaderRecord 30-102), `launch.rs` (create*/adopt/check/open 164-547),
      `accounts.rs` (627-709), `produce.rs` (submit/quote/produce/import/execute_publish/finish_pending 710-1170),
      `proofs.rs` (1171-1228).
    - demo/runtime.rs (965): `node.rs` (24-637) and `http.rs` (639-965).
    - Glob imports `use super::wire::*` at demo/runtime.rs:5, simulator/runtime.rs:6, simulator/host.rs:2,
      simulator/light.rs:5, simulator/metadata.rs:2, signed/runtime.rs:18 pull in functions named `hex`, `hash`,
      `number`, `pack`, `json`, `parse`, `rpc`, `client`. The two `wire` modules export the same names with different
      values (`REQUEST_CAP` 4 MiB at demo/wire.rs:6 vs 64 KiB at simulator/wire.rs:6; `HEADER_BATCH` 128 `u32` vs 64
      `usize`), and the glob is what let signed/runtime.rs:716 shadow `ProofRequest` unnoticed. Import by name.

15. **demo/runtime.rs role invariants by convention.** `chain: Option<MemoryChain>` and `signer: Option<SigningKey>`
    are unwrapped after a role check: `expect("producer")` 204, 250, 308, `expect("producer key")` 303,
    `expect("full engine")` 352, `expect("accepted head")` 173. `chain` is a `pub` field. An enum
    `Node { Producer { chain, signer }, Full { chain }, Light }` (what the later generations did) removes all six.
    **needs-care**.

16. simulator/host.rs:476-477 `first_key_value().unwrap()` + `remove` is `pop_first()`; 483 `c.config.unwrap()`
    depends on a check 350 lines of control flow away (127); 75 and 115 allocate `format!("0x{}", ..)` only because
    `unhex` insists on the prefix. simulator/wire.rs:377-401: the sixth limit is parsed by a copy of the loop body
    because its integer type differs - one generic `fn positive<T: FromStr + PartialOrd + Default>(raw)`.
    signed/pool.rs:461-465 `is_some_and` + `remove().expect("entry checked above")` -> `btree_map::Entry`. All **safe**.

### II.4 [T5] arkiv-db tests: durable / faults / KV; examples

*(paths relative to `crates/arkiv-db/`)*

1. **tests/storage_faults.rs:431-541 `calls()`** (109 lines, clippy `too_many_lines`). `rg -c '\.map\(\|r\| format!' tests/storage_faults.rs` -> 13 and `rg -c 'Budget::unlimited\(\)\.traced\(\)' tests/storage_faults.rs` -> 13, although the file defines `text()` (232-234) and `traced()` (236-238) for exactly this and `snapshot_reads()` (259-291) uses them. Fix: `fn exec(branch: &mut Branch, actor: OwnerId, op: WriteOp) -> Result<String, StorageError> { text(branch.try_execute(actor, op, traced())) }`; the eight `try_execute` closures become one line each, the rest use `text(..)`. Also replace the three `let traced = Budget::unlimited().traced();` locals that shadow the helper (368, 703, 783). About 60 lines. **safe**.
2. **tests/simulator_composition.rs, whole file** reads unlike the rest of the chunk: single-letter bindings throughout (`g`, `c`, `p`, `e`, `b`, `i`, `h`, and `s`/`t`/`k`/`n` in the `Overlay` impl), no blank line between the top-level fns at 26-90 or around the `Overlay` items at 305-311, a glob import `versioned::*` (line 19), and line 877 is 151 characters (a `matches!` with a guard that rustfmt will not break). Fix for 874-878: `let DurableError::Storage(error) = error else { panic!("{error:?}") }; assert_eq!(error.kind, StorageErrorKind::Corrupt); assert!(error.detail.contains("configured initial state"));`. Rename to `genesis`, `candidate`, `packet`, `engine`. **safe**.
3. **tests/streaming_import.rs**: no module doc; no blank lines at 8-9 and 14-15; a `use` block in the middle of the file (291-294); `std::sync::atomic::` spelled out 9 times instead of imported; the literal `InterruptCopy { store, calls: std::sync::atomic::AtomicUsize::new(0), fail_on }` written 5 times (120-124, 202-206, 220-224, 383-387, 397-401). Fix (if the double survives 1.1): `InterruptCopy::new(store, fail_on) -> Arc<Self>`, import `AtomicUsize` and `Ordering`, move the `use` up. ~20 lines. **safe**.
4. **tests/support/durable_history.rs:158-336 `run_seeded_history`** - 173 lines, cognitive complexity 31 (clippy). Split: `fn random_op(rng: &mut u64, height: u64) -> WriteOp` (182-224), `fn run_block(height, memory: &mut Engine, durable: &mut DurableEngine, store: &dyn VersionedStore, rng: &mut u64)` (167-305), `fn check_reopened(store: Arc<dyn VersionedStore>, memory: &Engine, cache_bytes: usize)` (310-334). Same file: no blank lines between items at 9-35, bindings `a`/`b`/`ca`/`cb`/`x`/`y`. **safe**.
5. **tests/durable.rs:244** - `use arkiv_db::{decode_exact, durable::EngineManifest, encode};` is the last line of the file. Move it into the import at lines 2-6. Also no blank lines at 11-37 and bindings `g`, `b`, `q`, `p`. **safe**.
6. **tests/rehash.rs:412-420** - builds a `Step` with `create(..)` (which hard-codes `ALICE`, line 337), then takes it apart with `let Step::Write(_, op) = step else { unreachable!() }` only to swap the owner. Fix: split `create` into `fn create_op(namespace, key, cells) -> WriteOp` and `fn create(..) -> Step { Step::Write(ALICE, create_op(..)) }`; line 412-420 becomes `Step::Write(owner, create_op(..))`. Same file: the 126-line test at 371 (clippy) splits into `fn base_block() -> Vec<Step>` (374-391) and `fn random_step(rng, live, next, revisions, owners) -> Step` (405-486); `arkiv_db::Limits::DEFAULT_INDEXED_LEN` (line 73) is fully qualified although `Limits` is imported (line 53). **safe**.
7. **tests/storage_faults.rs:377-415** - `for damage in ["missing", "corrupt"]` with `match damage { "missing" => .., _ => .. }` twice (381-391, 412-415). Fix: iterate `[StorageErrorKind::Missing, StorageErrorKind::Corrupt]` and match on the kind; the second match disappears. Line 657 `crafted(.., value: Vec<u8>)` is not consumed (clippy): take `&[u8]`, which removes `honest.clone()` at 718. **safe**.
8. **Hand-rolled temp directories that leak on panic**: tests/adopt.rs:42-43 + 64 (`temp_dir().join(format!("arkiv-adopt-it-{tag}-{pid}"))`, `let _ = remove_dir_all`, then `remove_dir_all(..).unwrap()` at the end) and tests/streaming_import.rs:352-354 + 429 (`create_dir(..).unwrap()` fails if a previous run with the same pid died). The unit tests already have the fix: `struct Temp(PathBuf)` with `Drop` at src/durable/adopt.rs:688-693. Put that guard in the shared support module and use it in both. **safe**.
9. **Same name, different thing** across files that would share a support module: `struct Faults` (tests/adopt.rs:337) vs `type Faults = FaultyStore<MemoryVersionedStore>` (tests/durable_failures.rs:34); `publish` (durable.rs:30 vs durable_retention.rs:70); `key` and `candidate` (table in 1.2). Settle the names when extracting. **needs-care**.
10. **clippy `redundant_clone`**, 16 sites: tests/adopt.rs:261 (two), 279, 281, 282, 293, 294; tests/durable_failures.rs:560, 596, 1394, 1444, 1557; tests/simulator_composition.rs:202, 274, 290; tests/rehash.rs:687. Remove the `.clone()` on the last use. **safe**.
11. **examples/**: blocks.rs `main` is 153 lines and records.rs `main` (line 93) is 281 lines with cognitive complexity 48 (clippy). records.rs already has natural seams at its banners: one fn per block (`create_shop`, `create_catalog`, `queries_after_block_2`, `patch_price_type`, `pinned_walk`, `history`, `expiry`) over a small context struct (`engine`, `blocks`, `shop`, the four cell names). `arkiv_db::NamespaceName` and `arkiv_db::Effect` are written fully qualified at blocks.rs:23, 30, 34, 37 and records.rs:112 next to an import list; blocks.rs:156 is 144 characters (split the `println!` at 155-157 in two); clippy `redundant_clone` at records.rs:299, 339, 357. **safe**.
12. **tests/adopt.rs:341** `apply_then_report: Mutex<bool>` and the `s`/`t`/`k`/`n`/`b`/`o` parameter names in the impl (344-381); gone if 1.1 is applied.

### II.4 [N4] arkiv-node deterministic core

*(`src/` = `crates/arkiv-node/src/`)*

**S1. `src/arkiv/mod.rs:394-663 execute_operation` - 270 lines, 18 `return Ok(..)` exits, 4 `expect`s and an
`unreachable!()`.** needs-care [CONSENSUS] (pure refactor)
Named split, in the order the fn already runs:
1. `load_current(branch, namespace, c, op)` 411-438: read, `project`, expired, authority.
2. `admit_expiry(branch, c, op, current)` 439-490 -> `(new, paid_until)`: resolve, owner-only, shortened, spill.
3. `build_write(branch, namespace, c, op, current, new)` 491-561 -> `(WriteOp, next_nonce)`.
4. `quote_rent(branch, c, ..)` 562-611: resulting cells, size cap, units, rent, affordability.
5. apply 612-629: `try_execute_host`, burn, nonce.
6. `reschedule(branch, namespace, old, after)` 630-654 (D6).
Give the phases `Result<T, Stop>` with a private `enum Stop { Rejected(OpOutcome), Fatal(ExecError) }` plus
`From<ExecError>`, `From<ArkivFault>`, `From<HostFault>`, so `?` replaces the 18 hand-written early returns. The
`expect`s at 509 ("create expiry"), 524 ("mutation entity"), 544 ("extension"), 556 ("mutation key") and
`_ => unreachable!()` at 552 exist only because phase results are carried as `Option` locals into one big `match`;
matching on `op` once per variant removes them. `quote_tx` 699-827 (122 lines) shrinks with D9.

**S2. `src/exec.rs` long methods (clippy: 117 / 108 / 106 / 118 lines).** needs-care [CONSENSUS] (pure refactor)
- `Executor::op` 1099-1234: three unrelated paths chained by `if let .. { return }`. Split `transfer_op` (1108-1134),
  `arkiv_op` (1135-1154), `write_op` (1155-1233); `op` becomes a `match`.
- `Executor::sweep` 796-907: `read_due_batch` (806-816), `expire_entry` (820-880), `check_totals` (882-905).
- `Executor::tx` 948-1056: `charge_intrinsic` (964-990), which also removes the second hand-built `TxOutcome`.
- `execute_block_inner` 560-697: the producer's defer decision (608-627) is a `fn defer_reason(..)`.

**S3. Dead or test-only code.** rg scope: `crates conformance scripts`.
- `src/arkiv/schema.rs:404-437 encode_natural_value` / `decode_natural_value` (34 lines): callers only in
  `src/arkiv/tests.rs` (748, 751, 752, 754). DELETE (with T4). needs-care
- `src/arkiv/schema.rs:320-338 encode_abi_value`: callers only in `src/arkiv/tests.rs`. Mark `#[cfg(test)]` or move.
- `src/arkiv/schema.rs:212-216`: the `seen` `BTreeSet` duplicate check cannot fire; 209 already requires strictly
  increasing names. DELETE 4 lines. safe
- `src/gas.rs:284-286 reference_fee_glm` (only `gas.rs:709-712`), `src/gas.rs:107-109 RentClock::block_period_ms`
  (only `gas.rs:436`), `src/value_json.rs:24-26 type_name` (only its own test, 256). DELETE. safe
- `src/gas.rs:48-52 block_storage_bound`: `pub const fn` whose only caller is the unit test at 412 (and the doc at
  36). Move into `mod tests`, or use it to define `DEFAULT_MAX_NEW_STORAGE_UNITS_PER_BLOCK`. safe
- `src/identity.rs:255 drop_pending`, `308 included_len`, `313 pending_len`: only `tests/identity.rs`.
  `src/params.rs:351 ChainSpec::protocol_version`: only `tests/genesis.rs:322`. Low priority.
- Over-wide visibility: `src/params.rs:248 check_params` (only caller 322), `src/validate.rs:177 check_expiry`
  (only 229), `src/gas.rs:56 rent_cap_blocks` and `21 DAY_MS` (internal only), `src/block.rs:61-71` six `*_DOMAIN`
  consts (used only inside `block.rs`), `src/accounts.rs:312 unsupported()` (single caller `exec.rs:1110`). safe
- Root re-exports nobody names: `PreparedBlock` (`src/lib.rs:58`; `demo/runtime.rs:267/309` use `prepare`/`finalize`
  without naming the type) and `OpFault` (`src/lib.rs:60`). safe

**S4. Needless clones and allocations.**
- `src/replay.rs:230` `let mut requests = self.requests.clone();` copies the whole inclusion index (every identity
  ever included) once per block to stay all-or-nothing, although `commit_block` already checks before mutating
  (`src/identity.rs:284`). Do `self.requests.check_block(..)?`, then `engine.publish(..)?`, then
  `self.requests.commit_block(..)` (cannot fail after the check). needs-care
- `src/replay.rs:220` `outcomes: executed.outcomes.clone()`: `PreparedBlock` stores `block` and the whole
  `ExecutedBlock`, but only `executed.candidate` is read again (240). Destructure, move `outcomes` into the block,
  keep `candidate`. safe
- `src/arkiv/mod.rs:563-567`: `cells.clone()` of the whole create (up to 128 KiB of values) only to measure it.
  Borrow: `let after: &[(CellName, Cell)] = match &write { CreateRecord { cells, .. } => cells, PatchRecord { .. } =>
  { patched_cells = patched(..); &patched_cells } _ => &[] };`. safe [CONSENSUS: none]
- `src/block.rs:633-640` (clippy: passed by value, not consumed): take the value by move,
  `let injected = expires_at.map(|value| (expires_at_name(), Cell::attribute(value)));` and
  `cells.iter().cloned().chain(injected).collect()` in the two arms; drops the `value.clone()` at 637. safe
- `src/arkiv/schema.rs:46-70 sdk_safe_name`: `name.to_ascii_lowercase()` allocates on every validated cell and scans
  an inline 18-word array; `RESERVED.iter().any(|w| name.eq_ignore_ascii_case(w))` with a `const RESERVED`. safe
- `src/accounts.rs:67, 121, 147`: `initial_supply()` re-sums up to 256 allocations on every settle and every rent
  burn. Cache it in `AccountConfig::new`/decode or compute once per block. needs-care

**S5. Magic numbers in consensus code.** safe unless noted
- Creation flags: `src/arkiv/mod.rs:134` `flags & !3`, `:430` `creation_flags & 2`, `:536` `creation_flags & 1`,
  `src/arkiv/schema.rs:230` `n<=3`. The prose says "creation flag 2". Name them
  (`FLAG_READONLY = 1`, `FLAG_PERMISSIONLESS_EXTEND = 2`, `FLAGS_MASK = 3`).
- ABI type ids as bare literals in `src/arkiv/schema.rs:285-315` (`7`, `8`, `1..=6 | 9 | 10`) and `419-427`
  (`1 => 1, 2 => 4, 3 => 8, 9 => 20`) although `#[repr(u8)] enum ArkivTypeId` sits at 8-19 of the same file and is
  only ever used value -> id. Add `ArkivTypeId::from_u8` and match on the enum. needs-care [CONSENSUS: ABI decode]
- Tags: `src/accounts.rs:24-36` (1, 2, 3), `:252/260` (account version 1), `:224` (`52` = 20 + 32),
  `:220/258` (`12`, `44`); `src/arkiv/mod.rs:180-232` (operation tags 1-5). `block.rs` names every tag (88-127).
- `src/gas.rs:137-145 schedule()`: the production gas schedule is 31 unlabelled positional weights on two lines.
  `crates/arkiv-db/src/meter.rs:227-258` (`PriceSchedule::FIXTURE`) puts one weight per line with its class name;
  do the same. [CONSENSUS: values unchanged]
- `src/gas.rs:129-134`: a hand-rolled const `gcd` only to reduce 20e9 / 86.4e6 to `6250 / 27`, a value the docs at
  116 and 132 already state. Two literal consts plus
  `const _: () = assert!(RATE_NUMERATOR * DAY_MS == RATE_DENOMINATOR * RENT_WEI_PER_UNIT_DAY);`.

**S6. Missing pin for the gas schedule.** `quote_is_exact_and_schedule_identity_is_pinned` (`src/gas.rs:708`) does not
pin it, and `rg 'SCHEDULE_ID|gas::schedule\(\)' crates/arkiv-node/tests` shows only users, no golden value. Add
`assert_eq!(digest("..", &encode(&schedule())), <golden>)` or the literal table. [CONSENSUS]

**S7. `map_err(|_| ..)` that discards the cause.**
- `src/arkiv/mod.rs:419` `Err(_) => return Err(corrupt())`: any engine rejection of the host's own read becomes
  "Arkiv committed state is malformed"; `src/exec.rs:1078-1082` keeps the reason for the same read
  (`Invariant::ExpiryReadRejected(reason)`). Also 322, 332, 424, 848, 858.
- `src/accounts.rs:87, 126, 152, 281, 288` all map to `corrupt()` = `StorageErrorKind::Corrupt, "account state"`.
  At 126/152/281 the dropped value is the host-map refusal that `src/exec.rs:761` reports as `Invariant::HostValue`:
  the same engine answer is a storage corruption in one file and a broken invariant in the other.
behaviour-affecting only for the error returned on corrupt state.

**S8. `panic!` in a pub fn: `src/block.rs:642-643 TxOp::to_write_op`** panics for `Arkiv` and `Transfer`. Both
callers return for those variants first (`src/exec.rs:1108/1135` before 1189, `src/validate.rs:189/192` before 232),
so it cannot fire today, but the type allows it. Return `Option<WriteOp>`, or split `TxOp` so the generic writes are
their own enum. needs-care

**S9. `src/accounts.rs:212` `e.u32(self.allocations.len() as u32)`** - unchecked cast in an encoder; the codec has
`Encoder::length(len)` (`crates/arkiv-db/src/codec.rs:302`). safe [CONSENSUS: same bytes for len <= 256]

**S10. Formatting and naming.** safe
- No blank line between items: `src/gas.rs` (15 boundaries, e.g. 83/84, 128/129, 145/146, 202/203, 279/280),
  `src/accounts.rs` (16, the whole file), `src/arkiv/mod.rs` (25), `src/arkiv/schema.rs` (16), `src/arkiv/tests.rs`
  (24, e.g. 73/74, 122/123, 157/158). Module doc runs straight into `use`/`mod` at `gas.rs:5/6`, `accounts.rs:2/3`,
  `arkiv/mod.rs:1/2`, `arkiv/schema.rs:1/2`.
- rustfmt gave up inside macros: `src/arkiv/schema.rs:221` `matches!(cell.value,Value::Key(k) if k==structural)`,
  `:230` `matches!(cell.value,Value::U64(n) if n<=3)`.
- Unit tail calls without `;` (clippy): `src/arkiv/mod.rs:44, 184, 190, 195, 200, 204, 550`, `schema.rs:327`.
- Full paths instead of `use`: `src/exec.rs` `crate::accounts::` 11x, `crate::gas::` 6x, `crate::arkiv::` 6x,
  `alloy_primitives::U256` 6x, `std::collections::BTreeSet` at 606 (while `BTreeMap` is imported at 54),
  `std::borrow::Cow` 648/651; `src/arkiv/mod.rs` `alloy_primitives::U256` 9x, `crate::gas::` 10x, `crate::exec::` 7x,
  `std::collections::` 129/695; `src/block.rs` `alloy_primitives::U256` 532, 787, 1351, 1372;
  `std::collections::BTreeSet::new()` at `validate.rs:354`, `identity.rs:262`, `schema.rs:212`.
- `encoder`/`decoder`/`formatter` (block, identity, params, validate) vs `e`/`d`/`f` (accounts, gas, arkiv).
- `src/arkiv/mod.rs:416-417`: the match binding `rejected` shadows the fn `rejected`, forcing `self::rejected(..)`.
- `src/arkiv/mod.rs:576` and `:630` compute "is a delete" twice (`matches!(write, ..)`, `matches!(op, ..)`).
- Enum variants and match arms out of tag order because later variants were prepended: `TxOp` (`Transfer` = 9,
  `Arkiv` = 8 first; `src/block.rs:525-603`, 701-776, 784-827), `AdminRejection` (`GasProfileLocked` = 4 first,
  923-937), `HostFault` (`Account` = 5, `Arkiv` = 4 first, 1090-1103), `OpOutcome` (`AccountRejected` = 7 first,
  1163-1207), `GenesisError` (`InvalidGasSchedule` first, `src/params.rs:63`, and its `Display` arms in a third order,
  107-148). Reorder to tag order so the tables read against `docs/block-format.md`; nothing derives `Ord`.
- `src/block.rs:609`: identical arm; fold `Self::Arkiv { namespace, .. }` into the or-pattern at 611-616.
- `src/block.rs:1166-1167`: `OpOutcome::AccountRejected` is the only variant whose fields have no doc.

### II.4 [S1] arkiv-db src: engine, typed, planner, record, layout, codec, meter

*(paths relative to `crates/arkiv-db/src/`)*

#### 4.1 Module sizes and split
engine.rs is 3140 lines holding seven concerns. Proposed `engine/` directory (line ranges of today's file):
- `engine/mod.rs` (~560): module docs 1-82; constants, `HostStateError`, `SnapshotId`, `EngineError` 117-261; `WorkingState` 263-329; `checked` 365-375; `SnapshotState`, `Engine` 421-635; `SealedCandidate` 637-692.
- `engine/staging.rs` (~80): `WorkingState::staging` 331-362, `Staging`, `MapSink` 377-419.
- `engine/snapshot.rs` (~620): `Snapshot` 694-1301, `index_consistent` 1303-1317.
- `engine/ops.rs` (~185): `WriteOp`, `Effect`, `Authority`, `Executed`, `Rejected`, `Metered`, result aliases 1338-1520.
- `engine/branch.rs` (~830): `Checkpoint`, `Saved` 1319-1336; `Branch` 1522-2336; the tests 2992-3140 (they read private `Branch` fields).
- `engine/write.rs` (~290): `zero_cost`, `admit`, `resolve`, `authorize`, `commit_namespace`, `apply` 2338-2625.
- `engine/read.rs` (~365): `finish_read` .. `read_terms` 2627-2990.
- `engine/explore.rs`: today's `src/explore.rs`, which removes the `#[path = "explore.rs"] pub mod explore;` at engine.rs:83-84 (see 5.2).
With 1.1, 1.2, 1.6, 1.7 applied, snapshot.rs and branch.rs land near 350-450 lines each. **needs-care** (moves only).
typed.rs (1443) splits naturally at its banners: admission 169-329 + 701-726 -> `typed/validate.rs`; index maintenance and writes 331-656 -> `typed/write.rs`; reads and evaluation 658-1435 stay. Optional. codec.rs (1487) is flat and fine once 1.3 is applied.

#### 4.2 Dead / unused public items (each verified with rg over `crates conformance scripts docs`)
- `Snapshot::hashed_nodes` engine.rs:873-887: no caller. `rg -n "hashed_nodes"` -> engine.rs definitions/doc links, and `tests/rehash.rs:187` which calls it on a `SealedCandidate` (`let sealed = branch.seal(); sealed.hashed_nodes()`). DELETE; move a 3-line doc to `SealedCandidate::hashed_nodes` (687-691), whose doc currently points at the dead one. -15. **safe**.
- `TypeTag::decimal_scale` record.rs:368-376: `rg "decimal_scale"` -> definition only. DELETE. -9. **safe**.
- `Receipt::is_applied` meter.rs:574-579: `rg "is_applied"` -> definition only. DELETE. -6. **safe**.
- `Budget::is_traced` meter.rs:390-393 and `Budget::is_interruptible` :395-398: `rg "is_traced|is_interruptible"` -> definitions only (`Meter::new` reads the fields directly, :609, :611). DELETE or use them in `Meter::new`. -8. **safe**.
- `Encoder::i64` codec.rs:282-285, `Decoder::i64` :385-388, `impl Encode/Decode for i64` :542-554: no production type encodes an `i64` (`rg "\.i64\("` outside codec.rs: none; every signed value goes through `term_bytes`). Only `tests/codec.rs:160-162` goldens use it, and the module doc lists it as a codec rule (:7-8). Decide: delete (22 lines + 3 golden lines + doc clause) or keep as spec. Not `[CONSENSUS]` today since nothing on chain uses it.
- `TypeTag::supports_prefix` record.rs:393-396: used only by `tests/records.rs:744`; the engine hard-codes `TypeTag::Str` (typed.rs:1123, planner.rs:303). Either use it or drop it.
- `RUN_WALK_CAP` planner.rs:105 is `pub(crate)` but used only inside planner.rs: make it private.
- `apply`, engine.rs:2454-2456: the `else { return Err(Rejection::Unauthorized.into()) }` of `let Authority::Owner(actor) = authority` cannot fire; `admit` (2364-2374) rejects every non-record op under `Authority::Host` before `apply` runs. Falls away if `create_namespace` takes `actor: OwnerId` after the split.

#### 4.3 `#[allow(clippy::too_many_arguments)]` - four sites
- engine.rs:2786 `read_record` (8 params): fixed by the `View` struct of 1.2.
- typed.rs:1350 `query` (8) and typed.rs:958 `open_run` (9): 23 fns in typed.rs and 9 in planner.rs take `loaders: &NamespaceLoaders`, most of them next to `state: &TypedState`; bundle the pair as a `Copy` struct `Index<'a> { state, loaders }` (-1 param everywhere, `query` drops to 7), and pass `open_run`'s `start, skip_start, end` as one run description (they already travel together in planner's `RunSpec`/`Shape`, planner.rs:397-410). Params struct is the obvious fix in all three.
- typed.rs:1324 `evaluate`: 7 parameters, which is the lint's default threshold (it fires above 7), so this `allow` looks unneeded. Switch the remaining ones to `#[expect(..)]` so a stale one is reported.

#### 4.4 Needless clones / allocations
- layout.rs (tests, clippy `redundant_clone`): :717 `encodings.clone()`, :942 `existence.clone()`, :949 `one.clone()`, :959 `red.clone()`, :1025 `key.clone()` - each is the value's last use; drop `.clone()`. **safe**.
- meter.rs:656 `let mut subtotals = [Cost::ZERO; OperationClass::ALL.len()];` - a 248-byte array is zeroed on every `charge`/`charge_group`, traced or not, only so the ledger branch (675-693) can look the subtotal up again. Compute `count * price` inside the ledger branch (overflow was already checked) and drop the array. It also removes a latent trap: a class listed twice in one group overwrites its own slot (:662), so the ledger subtotal would be wrong. **needs-care** `[CONSENSUS]` (spent and ledger values identical for today's callers; `meter.rs` tests cover).
- typed.rs:992 and :995: `bound.term_bytes().expect("ordered")` allocates a `Vec` for every term visited inside the `open_run` loop; hoist it above the loop as planner.rs:565-572 already does. **safe** (no charge involved).
- typed.rs:337-346 `terms_of` returns a fresh `Vec<TermKey>` (0 or 2 items) per cell in create/patch/delete; `Option<[TermKey; 2]>` avoids it. **safe**.
- typed.rs:497 `key: key.clone()` (the owned `key` is otherwise only borrowed; build the row inside the `and_then` closure - this is clippy's "passed by value, not consumed" at :463) and typed.rs:598 `key: row.key.clone()` (`row` is owned and not used after 597; move the field). O(1) `Arc` clones, so cosmetic. **safe**.
- engine.rs:1146 / :2173 `lower: Bound<Value>` taken by value and only borrowed (clippy). Public signature, so **needs-care** (call sites).

#### 4.5 Inconsistent or odd code
- engine.rs:896 `let handle = |root: B256, len: u64| (root, len);` - an identity closure that builds a tuple, used four times at 903-906. Write the tuples. **safe**.
- engine.rs:619-622: `let root = OnceLock::new(); if let Some(computed) = candidate.root.get() { let _ = root.set(*computed); }` -> `let root = candidate.root.clone();` (`SealedCandidate` already derives `Clone` with that field). **safe**.
- engine.rs:801-823 `prove_host` is written in a different hand from the rest of the file: single-letter `e` for both the error closure (807) and the encoder (808), `crate::Encoder` / `crate::StorageErrorKind::Corrupt` fully qualified although `StorageErrorKind` is imported (98) and `corrupt()` exists (110), `path.len() as u32` (811) where `Encoder::length` (codec.rs:302) does the checked conversion, and a bare wire version `e.u32(1)` (809) whose twin is `d.u32()? != 1` at proof.rs:1074. Name the constant and move the encoder next to `verify_host` in proof.rs, as `prove_eq`/`prove_range` already delegate. **needs-care** `[CONSENSUS]` (proof bytes are an RPC body; must stay identical).
- Same style drift at engine.rs:715-723 (`|e| corrupt(e.to_string())` three times, `crate::decode_exact(&crate::encode(header))`) and meter.rs:589 / :651 (`std::sync::Arc<std::sync::atomic::AtomicBool>`, `std::sync::atomic::Ordering::Relaxed` spelled out in a file that otherwise imports).
- typed.rs:352-358 `insert_membership(.., term: TermKey, ..)` takes the key by value and only borrows it (clippy :355), while its mirror `remove_membership` (382-388) takes `&TermKey`. Make both `&TermKey`. typed.rs:542 `let mut set = set;` where `create` writes `mut cells` in the signature (464). **safe**.
- engine.rs: `self.state.state` appears 22 times (`Snapshot.state: Arc<SnapshotState>`, `SnapshotState.state: WorkingState`). Rename one level (`inner` / `working`, as `Branch` already calls it). **safe**.
- engine.rs:157 free fn `count(len: u64) -> usize` shares its name with the `count` read on both handles and with `read_count` / `typed::count`. Rename (`to_usize`). **safe**.
- layout.rs: `.map_err(codec_error)` 12 times; `impl From<DecodeError> for CodecError` (allowed: `DecodeError` is local) turns them into `?` and removes `codec_error` (75-77) and the closure at 454. **safe**.

#### 4.6 Validation done by round trip
engine.rs:714-716: `Snapshot::from_header` validates a header it already holds by `decode_exact(&encode(header))` ("Strict decoding checks allocator/handle relations as well as format"), because the three checks live only inside `Decode for StateHeader` (state.rs:89-118). `Engine::from_state` (500-505) checks the format by hand and never checks `next_namespace > catalog_len`. Fix: `StateHeader::validate(&self) -> Result<(), DecodeError>` in state.rs, called from `decode_from` and from both constructors. **needs-care** for `from_header` (same accept set); **behaviour-affecting** for `from_state` only in that a test aid starts rejecting an inconsistent allocator.

### II.4 [N1] arkiv-node signed JSON-RPC / HTTP reads

*(paths relative to `crates/arkiv-node/`)*

#### S1. `rpc.rs:942-1268` `dispatch`: 319-line `match method` with inline bodies (emphasis b)

Yes: one `match method { .. }` from `:955` to `:1267`. Nine arms are one-line constants (`:956-964`); the rest are inline bodies of 5-71 lines. Four arms merge two methods and then re-test `method ==` inside the body, 11 times in total (`:969, 1017, 1024, 1116, 1152, 1175, 1186, 1195, 1198, 1207, 1225`).

Proposed split (arms become one line each; keep the method-name literals in `dispatch`, see the caveat):

| Arm (lines) | Becomes |
|---|---|
| `eth_getBalance` / `eth_getTransactionCount` (`:965-980`) | `balance(host, a)`, `transaction_count(host, a)` |
| `eth_getCode` (`:981-985`), `eth_getBlockByNumber` (`:986-993`), `eth_getBlockByHash` (`:994-1004`), `..CountByNumber` (`:1005-1010`), `..ByBlockNumberAndIndex` (`:1040-1048`) | `code`, `block_by_number`, `block_by_hash`, `block_transaction_count`, `transaction_by_index` |
| `eth_getTransactionByHash` / `eth_getTransactionReceipt` (`:1011-1039`, 2 inner `method ==`) | `transaction_by_hash(host, state, a)`, `transaction_receipt(host, state, a)` sharing `fn dropped(host, hash) -> Option<Error>` |
| `eth_sendRawTransaction` (`:1049-1055`) | `send_raw(host, a)` |
| `eth_estimateGas` (`:1056-1079`), `eth_call` (`:1080-1108`) | `estimate_gas`, `call`; both re-parse `to`/`data` from the call object that `simulation` parses again (`:1069-1070, 1082-1088` vs `:791-798`): parse once into a `Call { from, to, data, value, gas }` |
| `eth_newFilter` / `eth_newBlockFilter` (`:1110-1157`) | `new_filter(host, state, Some(spec))` / `new_filter(host, state, None)` |
| `eth_getFilterChanges` / `eth_getFilterLogs` (`:1163-1233`, 71 lines, 6 inner `method ==`) | `filter_poll(host, state, id, Poll::Changes | Poll::Logs)`; the block-hash branch (`:1172-1194`) is its own fn |
| `eth_feeHistory` (`:1234-1265`) | `fee_history(host, a)` |

The other two long functions:
- `rpc.rs:782-941` `simulation` (153): `parse_call` (`:788-828`), `result_keys(host, from, &ops)` (`:829-854`), `run(host, &spec, tx, mode) -> Result<TxOutcome>` incl. the `ExecError` mapping (`:862-912`), outcome -> error (`:913-930`), result encoding (`:931-940`).
- `rpc.rs:611-763` `revert_data` (144): `capacity_full_reason` (`:626-641`), `entity_before_op(host, tx, index, key, height) -> Result<(Option<owner>, Option<expiry>)>` (`:646-689`), and the fault -> `(signature, args)` table (`:690-757`).

Module split (rpc.rs is 1488 lines of code): `rpc/mod.rs` (envelope, `single`/`locked`/`detached`/`handle_request`/`http`, error mapping; today `:1269-1487`), `rpc/params.rs` (`:122-178`), `rpc/blocks.rs` (`index`, `locate`, `transaction`, `block_json`, `receipt`), `rpc/logs.rs` (`ExpiryReplay`, `block_logs`, `bloom`, `matches_log`, `logs`, `validate_filter`, filter state and methods; ~500 lines), `rpc/simulate.rs` (`revert_data`, `error_string`, `simulation`, call/estimate; ~330), `rpc/tests.rs`. Likewise `rpc_reads.rs` (1787 code): `mod.rs` (errors, planning), `page.rs` (`read_page`, `sorted_page`, scan fallback, `project`; `:729-1270`), `parse.rs` (`bounded`..`Parser`; `:1272-1786`), `tests.rs` (1440).

Caveat, **needs-care**: `state_docs.rs:120-135` (`reference_covers_every_dispatched_rpc_method`) does `include_str!("rpc.rs")` / `include_str!("rpc_reads.rs")`, takes the text from `fn dispatch(` to the next `"\nfn "` and collects quoted `eth_`/`net_`/`web3_`/`arkiv_` literals. The split works only if the method names stay as literals inside `fn dispatch` and the include paths are updated. Note it already works by accident in `rpc_reads.rs`: `dispatch` (`:282`) contains no method name; the slice runs on through `pub(super) fn detached` and `pub fn plan` (which do not match `"\nfn "`) until `fn read_budget` (`:377`). Making `plan` private would break the test.

#### S2. Minified / unformatted code (emphasis b)

All lines > 150 chars (`awk 'length > 150'`), production code first:
- `rpc.rs`: `213` (450, the whole transaction `json!`), `260` (764, the whole block `json!`), `475` (249, log), `1263` (230, feeHistory), `1312` (220, error envelope with a `match` inside the `json!`); tests `2040` (189), `2086` (163), `2221` (177).
- `state_api.rs`: `85` (333), `86` (341), `88` (174), `91` (174), `94` (236), `97` (684), `98` (615).
- `metrics.rs`: `115` (262), `330` (224), `333` (355) - format strings holding 3-4 exposition lines each.
- `rpc_ws.rs`: `234` (173), `246` (166), `252` (165).
- `rpc_reads.rs` (tests only): `1803` (174), `2082` (227), `2794` (195); production lines 101-150: `182`, `340`, `1244`.
- None in `ethereum.rs`, `range_api.rs` (one of 109), `reads.rs`, `state_docs.rs`.

Whole regions rustfmt gave up on (no spaces around operators, several statements per line):
- `state_api.rs:72-101`: the entire closure, 30 lines, e.g. `:75` `let Node::Full(host)=&*node else{return Err(Error("UnsupportedNodeRole"))};`. Worst offender of the chunk.
- `rpc_ws.rs:177-262`: the whole `tokio::select!` body (86 lines; clippy cognitive complexity 33/25).
- `range_api.rs:165-172` and `:178-183` inside the `spawn_blocking` closure.
- `rpc.rs:1363` (`matches!(&*node,Node::Full(h) if method=="eth_sendRawTransaction"&&!h.is_producer())`).

Cause and fix: rustfmt does not touch a `json!({..})` body and abandons the enclosing statement/closure when a line cannot fit; `select!` bodies are never formatted. So (1) hand-break every long `json!` one key per line (`rpc.rs:213, 260, 475, 1263`; `rpc_reads.rs:340, 1244`); (2) `rpc.rs:1312`: compute `(code, message)` first (see S4); (3) `state_api.rs`: name the closure body `fn explore(rt: &Shared, q: &Request, ns: Option<NamespaceId>, height: u64) -> Result<Json>` and give each JSON shape its own fn: `leaf_json(ExploredLeaf, &[u8])` (`:83-96`), `cell_json(name, cell)` (`:88-95`), `node_json` (`:97`), `composition_json(&StateHeader)` (in `:98`); import `inspection::{tag_name, typed_value}` instead of four full paths; (4) `rpc_ws.rs`: make the two `select!` arms one call each, `on_message(&mut socket, &rt, message, &mut subscriptions, &mut next_id) -> ControlFlow<()>` and `on_tick(..) -> ControlFlow<()>`, so rustfmt formats the bodies and `return`/`break`/`continue` become explicit. All **safe** (bodies unchanged) but **needs-care** for `rpc_ws.rs` (the `select!` body mixes 9 `return`, 6 `break` and 3 `continue`, with different meanings: close the socket, leave a loop, skip the tick).

Blank lines between items (known pattern, numbers only): `rpc.rs` 2 blank lines in 1488 code lines, `rpc_reads.rs` 6 in 1787, `ethereum.rs` 2 in 352, `metrics.rs` 3 in 622.

#### S3. Dead code and unused items

- `rpc.rs:1384` `_ => Err(Error("FullNodeRequired"))` is unreachable: `forward` (`:1362`) is already true for `Node::Light`, so the `else` branch only sees `Node::Full`. With it gone the `Option<Result<Value>>` + `Ok::<_, Error>(local)` + `local.transpose()` dance (`:1364-1394`) collapses: `let Node::Full(h) = &mut *node else { return Ok(None) }; if method == "eth_sendRawTransaction" && !h.is_producer() { return Ok(None) }` and the closure returns `Result<Option<Value>>`. Also `:1359` `let response_id = id;` is a rename only. 42 -> ~25 lines. **safe**.
- `rpc.rs:581-583`: the `blockHash` + `fromBlock`/`toBlock` check is unreachable; `validate_filter` at `:579` already rejected it (`:102-106`). DELETE. **safe**.
- `ethereum.rs:33, 179` `EthereumTransaction::gas_price` is written and never read (`rg "\.gas_price\b|gas_price:" crates conformance scripts`: only these two lines). DELETE the field. `:160` and `:179` spell `1_000_000_000u64` instead of `crate::accounts::GAS_PRICE_WEI` (`src/accounts.rs:10`). **safe**.
- `ethereum.rs:306` `call_ops(_genesis: &SignedGenesis, ..)`: unused parameter; two callers (`ethereum.rs:323`, `rpc.rs:816`). Drop it. **safe**.
- `pub` with no user outside `rpc.rs` (`rg -n "block_json|matches_log|rpc::logs|rpc::quantity" crates conformance scripts`: no hits outside the file): `rpc.rs:125` `quantity`, `:242` `block_json`, `:538` `matches_log`, `:578` `logs`. Make private. (`block_logs` and `hex` are used by `tests/signed_host.rs`; `validate_filter`, `handle_request`, `HISTORY_PRUNED_CODE` by `rpc_ws.rs`.) **safe**.
- `rpc_reads.rs:830-836`: the "response exceeds 8 MiB" check cannot fire: `rows_within` (`:781`) already bounded the rows to `PAGE_BYTES = RESPONSE_CAP - 4096` and the envelope adds under 100 bytes. It costs a second full serialization of up to 8 MiB; `rpc.rs:1450` (or `:1435`) serializes a third time for the same cap and axum a fourth. `rows_within` (`:808-818`) also allocates a `Vec` per row only to take its length: count with a `std::io::Write` sink (`simulator/wire.rs:43-66` has one). **needs-care**.
- `rpc_reads.rs:502-504` `read_error` is a one-line wrapper over `rejection(&error.reason)` taking its argument by value (clippy `:502`); inline or take `&Rejected`.

#### S4. Stringly-typed error consequences in this chunk (emphasis d)

1. **Error detail smuggled past `Error(&'static str)` through mutable state.** `dispatch` returns `Result<Value, Error>`, which cannot carry a `QueryError` or revert data, so: `rpc.rs:946-951` stores the query error in `RpcState::query_error` and returns the placeholder `Error("QueryRejected")` (constructed here, matched nowhere: `rg '"QueryRejected"' crates` = 1 hit); `:928` writes revert data through an out-parameter `revert: &mut Option<String>` into `RpcState::revert_data`; `:943-944` must reset both on every call; `:1375-1380` builds an error response from the placeholder and then throws it away to build the real one. Fix: `dispatch` (and `simulation`) return `Result<Value, Failure>` with `enum Failure { Node(Error), Query(QueryError), Reverted(Option<String>) }` (`ReadError` at `rpc_reads.rs:192` already is the first two). Removes two `RpcState` fields (`:38-40`), the resets, the out-parameter (5 test call sites pass `&mut None`: `rpc.rs:1731, 1759, 1764, 1782, 2198`), the placeholder code and the build-then-discard. **needs-care**, bodies unchanged.
2. **`rpc.rs:1029-1035`: drop reason string -> error code string, with a wrong default.** `host.pool.dropped.reason(hash)` returns `&'static str`; `"ExceedsStorageCap" => "TransactionDroppedExceedsStorageCap", _ => "TransactionDroppedInsufficientBalance"`. Only two reasons exist today (`src/signed/host.rs:1007`, `src/signed/pool.rs:316`), but any third one will be reported to clients as "insufficient funds". Then `:1302-1307` maps those two codes to messages that are `"transaction dropped: "` + the messages of `:1298-1301`. Fix: a `DropReason` enum in `pool.rs` with `fn message(self)`, matched exhaustively. Interim: make the second arm `"InsufficientBalance" =>` and add an explicit fallback message. **behaviour-affecting** only for future reasons.
3. **`rpc.rs:1297-1312`: two `match e.0` over the same string in one function** (message at `:1297-1309`, numeric code inside the `json!` at `:1312`), plus the `"HistoryPruned"` special case at `:1287`. One `fn code_and_message(e: &Error) -> (i64, &'static str)`; `:1312` becomes a plain envelope. **safe**.
4. **`rpc.rs:1480`: HTTP 429 is decided by `result["error"]["message"] == "ServerBusy"`**, i.e. by the display text that `:1297-1309` exists to rewrite. Giving `ServerBusy` a friendlier message would silently drop the 429. Compare `["error"]["code"] == SERVER_BUSY_CODE`. **safe**.
5. **Same condition, two spellings.** Value sent to the Arkiv address: `"NonpayableCall"` (`rpc.rs:814`) vs `"NonzeroArkivValue"` (`ethereum.rs:321`). Calldata to another address: `"UnsupportedContractCall"` (`rpc.rs:819, 1090`) vs `"UnsupportedEthereumCall"` (`ethereum.rs:326`). So `eth_estimateGas` and `eth_sendRawTransaction` name the same mistake differently. Falls out of D4 (`ops_for`). `[CONSENSUS]` **behaviour-affecting**.
6. **`ethereum.rs:160`: wrong code.** `if parity > 1 || max_fee < 1 gwei { return Err(Error("GasPriceBelowMinimum")) }`: an EIP-1559 transaction with `yParity > 1` is refused as a gas-price problem. Split the condition; `parity > 1` is `bad()`. **behaviour-affecting** `[CONSENSUS]` (error text only; acceptance unchanged).
7. **Control flow by matching error strings**: `rpc.rs:987-991` (`Err(Error("UnknownBlock")) => return Ok(Value::Null)`) and `:201-204` (`Ok(()) | Err(Error("RpcIndexCapacity")) => ()`). `at` could return `Result<Option<u64>>`; `index` could return `Result<bool>` ("complete"). **safe**.
8. **Cause discarded**: `rpc.rs:281, 324, 654` map the engine's `Rejected` (which includes `OutOfBudget` for the 25 M-unit read) to `Error("StorageCorrupt")`; 14 `Error("StorageCorrupt")` in `rpc.rs` non-test code, several of them for conditions that are not corruption (`:445` an operation without a key, `:514` `as_object_mut` on a value built two lines earlier). Use `Error::from` where a conversion exists (as `rpc_reads.rs:407, 438` do) and remove the impossible ones (`:514`: assign with `v["status"] = ..` as `transaction` does at `:216-238`, instead of ten `obj.insert("..".into(), ..)`).
9. `reads.rs:58` `DEADLINE_EXCEEDED` is the only code with a constant; `"ServerBusy"` is a bare literal constructed at `reads.rs:153` and matched at `rpc.rs:1312`, `rpc.rs:1480`, `metrics.rs:61`.

#### S5. `unreachable!` / `expect` / `unwrap` in RPC paths

- `rpc.rs:468` `_ => unreachable!()` (clippy: wildcard matches a single variant). Write `ArkivOperation::Create { .. } => unreachable!("handled above")`: a sixth `ArkivOperation` variant then fails to compile instead of panicking (with the `RpcState` mutex held, see the `expect` below) when the first such block is read. **safe**.
- `rpc_reads.rs:1265` `_ => unreachable!()` in `project`: `FIELDS` is a `&[&str]` matched by string, so a field added to the list without an arm panics at request time. A 9-variant `enum Field` with `ALL` and `name()` makes the match exhaustive and removes the `selected[*field]` string indexing. **safe**.
- `rpc.rs:901` `.expect("one simulated transaction")` runs inside `dispatch`, i.e. while the `RpcState` mutex guard of `:1367-1370` is alive; a panic there poisons it and every later locked method answers `StorageUnavailable`. `.ok_or(Error("SimulationFailed"))`. Cannot fire today. **safe**.
- `rpc_reads.rs:558, 561` `v.as_object().unwrap()` twice right after `v.is_object()`: `if let Some(map) = v.as_object()`. `:582` `fields.as_object_mut().unwrap()`: validate into a `serde_json::Map` once. `rpc_ws.rs:92` (`params[0].as_str().unwrap()` after `is_string()`) and `:104` (`as_object().unwrap()` after `is_object()`): `let [Value::String(id)] = params.as_slice() else { .. }`. None can fire; all are check-then-unwrap. **safe**.
- `ethereum.rs:253` `expect("create has no tombstones")` is sound (`decode_protocol_triples(_, false)` returns `Err` for a tombstone, `src/arkiv/schema.rs:375-378`); listed only for completeness.

#### S6. Smaller items

- `rpc.rs:1250` (clippy "sequence of operators looks suspiciously like a bug", emphasis c): **not a bug.** `!n.is_finite() || !(0.0..=100.0).contains(&n) || n < prev` is the intended check (finite, in range, non-decreasing); the nursery lint `suspicious_operation_groupings` misfires on `n.is_finite .. contains(&n)` and its suggested `contains(&is_finite)` does not compile. The only real simplification: `!n.is_finite()` is implied by the range test (NaN and infinities are outside `0.0..=100.0`). Drop it or `#[allow]` with a reason. **safe**.
- `rpc_reads.rs:2585` and `:3003` (emphasis c): both in tests, both trivial; see section 2.
- `rpc.rs:312` `fn expiry(&mut self, ..)` never mutates (clippy `:312`): `&self`.
- `rpc.rs:913` `let out = &outcome;` alias; `rpc.rs:878, 889` fine.
- `ethereum.rs:108-159`: a 9-tuple `(chain_id, nonce, max_fee, priority, gas_limit, to_idx, sig_idx, parity, signing)` returned from an `if typed {..} else {..}`, with field positions as bare `5, 10` / `3, 7` and counts `12` / `9` (clippy: 105/100 lines). Two functions `typed_fields(&[Item]) -> Result<Unsigned>` / `legacy_fields(..)` returning a small struct, each headed by the RLP layout it reads. **needs-care** `[CONSENSUS]` (transaction authentication; `tests/ethereum_transactions.rs` covers both forms).
- `rpc_reads.rs:278` `static CURSORS: OnceLock<Mutex<Cursors>>` with `get_or_init(Default::default)` at `:702-705` and `:838-841`: `Mutex::new`, `BTreeMap::new` and `VecDeque::new` are `const`, so `static CURSORS: Mutex<Cursors> = Mutex::new(Cursors::new());` drops the `OnceLock`. **safe**.
- `rpc_reads.rs:359` `9_007_199_254_740_991`: name it (`MAX_SAFE_INTEGER = (1 << 53) - 1`). Limit messages hard-code their constants: "64 groups or 256 literals" (`:1276, 1283, 1290, 1304`), "8192 bytes" (`:1477`), "32 levels" (`:1672`), "64 predicates" (`:1689`), "8 MiB" (`:784, 790, 835`) beside `GROUP_CAP`, `LITERAL_CAP`, `QUERY_BYTES`, `MAX_DEPTH`, `MAX_PREDICATES`, `RESPONSE_CAP`. Changing a constant leaves the message wrong.
- Unnamed limits in `rpc.rs`: `64` addresses / alternatives (`:66, 86`), `4` topics (`:78`), batch `64` (`:1427`), feeHistory `1024` (`:1235`) and `100` (`:1244`), estimate margin `gas / 10 + 1000` (`:1075-1076`, restated in the test at `:1760`), body timeout `5 s` (`:1459`); `rpc_ws.rs:24` `16` connections, `:219` `+ 7` blocks per tick.
- Naming collisions across sibling modules that make call sites hard to read: `quantity` (`rpc.rs:125` formats, `rpc_reads.rs:486` parses), `read_error` (`rpc.rs:1276` builds a response, `rpc_reads.rs:502` converts a rejection), `detached` (`rpc.rs:1400` runs one, `rpc_reads.rs:286` is a predicate), `dispatch` (`rpc.rs:942`, `rpc_reads.rs:282`, `rpc_ws.rs:65`), `project` (`rpc_reads.rs:1228` vs `arkiv::project`), and `limits` (`rpc_reads.rs:207` fn vs the `limits: Limits` field, forcing `self::limits(..)` at `:784, 789`). Rename: `is_detached`, `over_limit`, `rejection_error`.
- `rpc_reads.rs:331-342`: the `arkiv_getBlockTiming` arm is an inline closure with a 142-char `json!` inside `plan`; extract `fn block_timing(host: &Host) -> Result<Json>`.

### II.4 [NT1] arkiv-node tests: core

*(`T/` = `crates/arkiv-node/tests/`)*

1. `T/support/mod.rs:6` `#![allow(dead_code)]` (legitimate: each test binary uses a subset) hides two helpers nobody uses:
   - `scan` (:220-226): `rg -n "(^|[^._a-zA-Z0-9])scan\(" T/{replay,validation,selection,block_format,block_model,exec,genesis,identity}.rs` and `rg "support::scan" T/` -> no hits.
   - `smallest_u64` (:279-297): `rg -n smallest_u64 --glob '!target' --glob '!vendor' .` -> only its definition.
   DELETE both, with the imports only they need (`std::ops::Bound` :8, `Entry`, `TypeTag` in :11-12). 26 lines + imports. **safe**.

2. Module too big: `T/block_model.rs` 2246 lines, with `T/support/block_roots.rs` attached by `#[path]` in mid-file (:2085-2086). block_roots.rs lives in `support/` but is not support: it does `use super::{CellKind, Model, Value, Weights}` (:3), so it compiles only as a child of block_model.rs, and it pulls in another crate's test code with `#[path = "../../../arkiv-db-trie/tests/support/mod.rs"]` (:6-7). Proposed split, as a directory test target `T/block_model/`: `main.rs` (constants, `verify`, the three tests, ~560), `weights.rs` (:113-375, 263), `model.rs` (:377-1188, 811), `roots.rs` (block_roots.rs, 197), `generator.rs` (:1393-1945, 553). **needs-care**.

3. Giant functions (clippy `too_many_lines`) with an obvious split:
   - `T/gas_execution.rs:719-905` `named_namespace_entities_preserve_rent_authority_proofs_and_expiry`, 185 lines, five subjects in the name. Split on a fixture `fn bricks_chain() -> (Host, Host, NamespaceId, RecordKey)`: (a) create pays rent and proves (:745-823), (b) owner authority and unsupported generic ops (:824-887), (c) expiry and follower replay (:888-904). Its function-local imports are redundant: `use arkiv_db::{Cell, NamespaceId, NamespaceName, OwnerId};` (:721) re-imports `Cell` and `OwnerId` from :2, and `use arkiv_node::{HostFault, arkiv::ArkivFault};` (:722) re-imports both from :5 and :7; `use arkiv_db::proof::{..}` appears at :633 and :720. Hoist to the top. **needs-care**.
   - `T/block_model.rs:1757-1945` `generate`, 172 lines: extract `fn scripted(generator, work, height) -> Vec<(OwnerId, Vec<TxOp>)>` for the three scripted scenarios (:1795-1898). Then :1904-1907 (`scripted.first()` + `.clone()` + `scripted.remove(0)`) becomes a `VecDeque::pop_front()` or an iterator `.next()`. **safe**.
   - `T/block_model.rs:2088-2246` demo test, 156 lines, five loop levels. `snap` (:2171), `accepted` (:2177), `attribute` (:2178) and `expected` (:2194-2211) are recomputed inside `for limit in [1, 3, 64]` although none depends on `limit`, and `expected` depends only on `model`. Extract `fn expected_group_rows(model: &Model)`, `fn proven_pages(snap, request, limits)`, hoist the rest. The two fulls and two lights are identical replicas (head equality asserted at :2154), so `for full in &fulls { for light in &lights {` multiplies the proof walks by four for no coverage; `fulls.iter().zip(&lights)` does the same job. **needs-care** (test runtime and shape).
   - `T/block_model.rs:763-908` `Model::engine`, 138 lines: move the `DeleteRecord` arm (:871-891) to `delete_record_charges` beside `create_charges` / `patch_charges`, and the four raw/namespace arms to `raw_charges`. **safe**.
   - `T/genesis.rs:126-296` `every_invariant_is_checked`, 162 lines: split at :253 into `chain_params_invariants` and `limits_invariants`; fold the lone `max_ops_per_tx: 64` acceptance (:243-252) into the accepted list at :229-240. **safe**.
   - `T/replay.rs:1172-1289` `replay_rejects_invalid_blocks`, 105 lines: add `let rejected = |error| Some(ReplayError::Block { height: 2, error });` and express `invalid` (:1192-1197) and the four literal wrappers (:1248-1251, :1258-1261, :1268-1271, :1279-1285) through it. ~12 lines. **safe**.
   - `T/replay.rs:533-684` `expiry_at_capacity`, 138 lines, cognitive complexity 26: leave it. It is one five-block golden fixture; splitting means re-recording `GOLDEN_CAPACITY_1..5`. kept: pins golden hashes.

4. Leftover debug output: `T/gas_execution.rs:163` `println!("1KiB {days} days: {gas} gas");`. DELETE. **safe**.

5. Needless clones:
   - clippy `redundant_clone` (each verified as the last use): `T/validation.rs:46` (`let mut full = valid;`), `T/identity.rs:193`, `T/selection.rs:49`, `T/gas_execution.rs:475`.
   - `T/replay.rs:495`, `:778`, `:853`, `:940` clone a whole `BlockOutcomes`, `:670` a whole `Block`, `:451-468` a `BlockOutcomes`, only to read one field in the next statement. `assert_eq!(fixture.empty().outcomes.expiry, vec![..]);` needs no clone - :937 already does exactly that.
   - `T/replay.rs:153` converts the golden `&[&str]` to `Vec<String>` before comparing; `Vec<String>` compares with `&[&str]` directly. DELETE the line.
   - `T/block_model.rs:620` clones a record's cells to call `self.delete_charges(&cells)`; both borrows are shared, the clone is not needed. `:856`, `:869` `injected.clone()` because `create_charges` / `patch_charges` take `Option<Value>` by value; take `Option<&Value>`.
   **safe**.

6. `#[allow]` hiding something:
   - `T/block_model.rs:910` and `:954` `#[allow(clippy::type_complexity)]`: the triple `(Vec<u64>, Option<(usize, Rejection)>, Option<Effect>)` is spelled out three times (:771, :918, :963). Give it a name (`struct Plan { charges, semantic, effect }` or a type alias) and both allows go.
   - `T/block_model.rs:954` `clippy::too_many_arguments` is dead: `patch_charges` has 7 parameters including `&self`, the default threshold is 7, and `crates/arkiv-node/clippy.toml` sets no other.
   **safe**.

7. Dead coverage counter: `T/block_model.rs:1451-1453` notes "budget spent exactly", but the required list (:2009-2038) never asks for it (it asks for "exact budget committed", :2017). DELETE the note or require it. Likewise the `AccountRejected` arm label (:1470) can never be produced by this generator. **safe**.

8. clippy "identical match arms": `T/block_model.rs:667` and `:669` (`CreateRecord` and `DeleteRecord` both give `(Some(*namespace), Some(key), false)`). Merge with `|`; better, since namespace and key are always both present or both absent, make it `let target: Option<(NamespaceId, &RecordKey)>` plus `is_patch`. **safe**.

9. Fully qualified paths for things the file already imports, or imports once it is asked to:
   - `T/block_format.rs:638`, `:647` `alloy_primitives::U256::` (U256 imported at :16).
   - `T/exec.rs:316` `arkiv_db::Limits` (imported at :13; used unqualified at :403); `:259` `RecordKey::new(b"h").expect("key")` where :210 uses support's `key(b"h")` - then the `RecordKey` import (:14) goes.
   - `T/gas_execution.rs:131` `arkiv_db::Cell` (imported :2); 9 `arkiv_db::` and 5 `arkiv_node::` qualified uses in total (:506, :623-628, :855, :983-994, ...).
   - `T/accounts.rs`: 12 `arkiv_db::` qualified uses (`arkiv_db::proof::verify_host` x5 at :265, :417, :421, :422, :425; `arkiv_db::RecordKey` :294, :322; `arkiv_db::Cell` :103, :107; :446, :614, :645).
   - `T/replay.rs:132`, `:535`, `:542`, `:1105`, `:1230`, `:1241`.
   **safe**.

10. No blank lines between items, unlike every other file of the chunk (count = item starts directly after a closing `}`): `T/gas_execution.rs` 26 (everything from :18 to :631, and :921), `T/accounts.rs` 18 (:21 to :502), `T/support/block_roots.rs` 8 (the whole file). In both test files the last few tests (accounts.rs :560, :600; gas_execution.rs :620 on) do have blank lines, so the files are inconsistent with themselves. No line over 120 characters in the chunk.

11. Small leftovers, one line each, all **safe**:
    - `T/block_model.rs:1549` `let names = NAMES;` (a copy of a const, used once at :1553).
    - `T/block_model.rs:1647` / `:1656` `let set = ..; ... let mut set = set;` -> declare it `mut`.
    - `T/accounts.rs:383` `let mut signed = signed;`; `:428` `let tx = tx(..)` shadows the helper fn; `:445` and `:448` build `OwnerId::new([1; 20])` twice (define `a`, `b` first).
    - `T/selection.rs:141-147` builds `one_more` even when `taken == 2` and needs `.min(2)` to survive it; build it inside the `if taken < 2`.
    - `T/replay.rs:604`, `:611`, `:612`, `:844`, `:851` `assert!(matches!(..status, TxStatus::Committed))` while :910 uses `assert_eq!` on the same `PartialEq` type; `assert_eq!` prints the status on failure.
    - `T/replay.rs:766-774` writes a `TxOutcome` literal because `applied` hard-codes schedule 1; `committed(520, vec![OpOutcome::Applied { .. receipt(2, 520, ..) }])` is 4 lines shorter.
    - `T/replay.rs:1076` clones `second` to push it, then keeps using the original; assert first, push last.

### II.4 [NT2] arkiv-node tests: signed

*(paths relative to `crates/arkiv-node/tests/`)*

#### 4.1 Giant test functions (emphasis c) - split or keep

| fn | lines | verdict |
|---|---|---|
| signed_http.rs:185 `external_admission_full_replay_and_light_paginated_proofs` | 307 | SPLIT. Four unrelated stages share one chain: status capabilities (:225-243, needs no chain), admission states (:244-356), body replication to a full and header sync to a light with the `bodies == 2` counter (:357-406, :488-492), light pagination/proof/classify/text (:407-487). Fixture: `producer_chain` with three creates plus the two counting middlewares (:193-219). The `bodies` counter must stay in the replication test and be re-asserted after the light reads (:457). needs-care |
| signed_http.rs:718 `streamed_submission_caps_private_auth_and_bounded_jobs` | 112 | SPLIT in three: private-router auth (:729-759), streamed body caps (:760-779), admission permits (:780-835). The last is the only timing-dependent part of the file; on its own a flake is attributable. safe |
| signed_http.rs:1148 `range_inspection_exposes_only_complete_authenticated_branching_witnesses` | 163 | KEEP the core (:1176-1264 is one pinned response and its tree walk) with the empty-range table (:1265-1279); move :1280-1309 out (section 2) and :1310-1314 with it |
| signed_http.rs:1318 `state_explorer_walks_pinned_real_nodes_and_nested_postings` | 141 | KEEP as a scenario (catalog -> maps -> terms -> postings is one traversal). Extract `explore(client, url, body) -> Value` for the four "POST, assert success, parse" blocks (:1329-1337, :1358-1366, :1372-1380, :1405-1413) and split the refusals (:1427-1459) into their own test. -25 |
| signed_host.rs:168 `signed_lifecycle_replays_bootstrap_ownership_failures_and_native_proofs` | 116 | KEEP: each step needs the state the previous one left |
| signed_host.rs:980 `creates_sharing_a_lifetime_fill_a_height_and_spill_to_the_next` | 171 (cognitive complexity 40) | KEEP as a scenario, its doc comment (:972-978) is the table of contents; shrink with `eq_at_head`, the shared fill loop and `entity_key` (1.4 items 6, 16, 18) |
| signed_host.rs:1246 `expiry_sweep_timing` | 140 | It is an `#[ignore]` benchmark that prints a table (:1288, :1377). `kind: &str` is matched in three places (:1263-1267, :1313-1317, :1334-1338); make it one table of `struct Shape { name, strings, len, body, per_tx, per_block, budget }`, reuse `chain`/`tx_on`, and drop the temp dir with a guard (:1388 runs only on success). crates/arkiv-node/examples/ already holds `gas_calibration.rs` if it should leave the test binary |
| ethereum_transactions.rs:172 `ethereum_original_auth_survives_mining_and_independent_full_replay` | 105 | SPLIT at :226: replay (:191-225) and "query the UDC entity back" (:226-279) share only `mined_eip1559() -> (Host, Host, SignedTx)` |
| ethereum_transactions.rs:299 `sdk_permanent_create_is_stored_at_the_sentinel` | 171 | SPLIT in three: the permanent create, sweep, receipt, rent and estimate (:311-392, :432-469); the SDK 1.0.1 query shapes (:393-431, section 2); status capabilities (:470-484, needs no chain) |
| arkiv_profile.rs:63 `lifecycle_history_full_replay_header_only_restart_and_typed_proofs` | 113 | KEEP; its two tables (:106-136, :145-158) are already the right shape |
| arkiv_profile.rs:247 `http_light_inspection_is_typed_bounded_and_independently_verifiable` | 179 | SPLIT by endpoint over one `served_light()` fixture: status (:291-310), inspect (:311-343), classify (:344-369), text (:370-426). With `post_json` alone it drops to about 110 lines |
| signed_protocol.rs:136 `preimages_are_explicit_and_golden` | 104 | KEEP (golden preimages) `[CONSENSUS]` |
| signed_protocol.rs:357 | 122 | table, 1.4 item 1 |

#### 4.2 Redundant clones (emphasis e; each checked: the value is not used afterwards)

- signed_host.rs:198 `create_tx.clone()`, :319 `failure.clone()`, :381 `included.clone()`, :643 `state.clone()`
- signed_history.rs:184 `encoded.clone()` (`let mut trailing = encoded;`)
- signed_protocol.rs:476 `tx.clone()`, :497 `env.clone()`; `roundtrip<T>(value: T)` (:245) never consumes its argument: take `&T` and `g.clone()` (:267), `header.clone()` (:272) go too
- signed_http.rs:448 `first = page.clone()` keeps a whole page to read `first["continuation"]` at :459; store the continuation
- signed_host.rs:949-969 and :1083-1085: `logged_expiries` returns `(String, String, u64)` whose first field is always `"created"` or `"extended"`; `&'static str` removes five `.to_owned()`
- signed_host.rs:1110-1121: sorts `proven`, sorts `expected`, then re-collects `proven` to compare; collect both into `BTreeSet<Vec<u8>>`
All safe.

#### 4.3 Lines rustfmt gave up on (> 150 chars) and unformatted chains

signed_http.rs:481 (195), :484 (203), :767 (166), :784 (188), :1328 (182); signed_retention.rs:129 (202); ethereum_transactions.rs:228 (190), :375 (178), :458 (159).
Also unformatted though shorter: arkiv_profile.rs:475-477 (`.send().await.unwrap();` on one line), signed_retention.rs:298, :340.
- signed_http.rs:481: the JSON literal is `text_query()` (:551-553) character for character; :484 is `text_query()` with `"query"` replaced. Move `text_query` above the first test and use it. safe.
- signed_http.rs:1328: closure `read` with an inline `if map=="catalog"{..}else{..}`; make it a fn.
- signed_retention.rs:129: build the expected error with a multi-line `json!`.

#### 4.4 Items run together without blank lines (known workspace item; counts of an item starting on the line after a closing `}`)

signed_reads.rs 20, signed_http.rs 19 (:39-182, :495-557), signed_host.rs 17 (:25-165, :476-479, :883-970, :1389-1390), signed_protocol.rs 8,
signed_retention.rs 6, signed_pool.rs 5, ethereum_transactions.rs 5 (:10-22: `mod`, three fns and a `#[test]` with no blank line), signed_history.rs 4, arkiv_profile.rs 3.

#### 4.5 Smaller items

- ethereum_transactions.rs: `arkiv_node::simulator::wire::hex` spelled in full 5x (:247, :275, :366, :433, :458) instead of one `use`;
  `fn bytes` (:14-16) has one caller (:20); :458 hard-codes `"0x4400000000000000000000000000000000000044"` where signed_retention.rs:140 uses
  `hex(&ARKIV_ADDRESS)` (crates/arkiv-node/src/arkiv/mod.rs:242); function-local `use` lists (:173-176, :300-309) repeat each other (`Options`, `MemoryVersionedStore`, `History`, `Host`, `Arc`) and belong at the top. safe.
- signed_protocol.rs: paths spelled in full although `protocol::*` and `arkiv_db::{..}` are imported: :110 `arkiv_node::signed::protocol::default_native_genesis`
  (used unqualified at :26), `arkiv_node::arkiv::entity_namespace()` :363, :381, `arkiv_db::Cell` :371, :375, :439, :449, :458, `arkiv_db::RecordKey` :383, :467, `arkiv_db::NamespaceId` :412.
  Same in signed_pool.rs:361-371 and signed_http.rs:115, :119, :1163. safe.
- signed_reads.rs:150 `struct Node2` - named around the imported `runtime::Node`; call it `Served`. :180 `limits: Default::default()` (clippy) -> `SignedLimits::default()`. :638 `const BLOCKS` declared mid-function.
- signed_reads.rs:434-438, :529, :625-629, :713 - four `eprintln!` timing reports. The numbers that matter are already in the assertion messages (:446, :494, :571, :586); delete, or keep the one at :625 if the walk sizes are wanted with `--nocapture`.
- signed_pool.rs:32-44 `trait Admit` with one impl, only to write `pool.admit(..)`; a free `fn admit(pool: &mut Pool, tx: SignedTx, next: u64)` says the same. :391 uses `16 * 1024 * 1024` where :379 and :383 use `MAX_BYTES`.
- `genesis()` is rebuilt (three signer derivations, account config, validation) on every use: 27 calls in signed_http.rs, 18 in signed_host.rs, where `tx` calls it twice per transaction (:73, :76). Pass `&SignedGenesis` (as `tx_on` and signed_pool.rs `tx` already do) or keep one `LazyLock`.
- signed_host.rs:1137-1142 `match next { E => 128, n if n == E + 1 => 128, n if n == E + 2 => 2, _ => 0 }` -> `match next.checked_sub(E) { Some(0 | 1) => 128, Some(2) => 2, _ => 0 }`.
- signed_http.rs:1560 `g.clone()` exists only because the certificate is built in a later argument (:1562); sign first, then move `g`.

### II.4 [T4] arkiv-db tests: codec / namespaces / blocks / rollback / roots / proofs

*(paths relative to `crates/arkiv-db/tests/`)*

1. eq_proofs.rs:1214 - `use arkiv_db::{decode_exact, encode};` is the last line of the file, after every test. Move into the import block at 6-14. **safe**
2. range_proofs.rs:873, 885-886, 894-898 - `Instant::now()` twice and an `eprintln!("1000 rows distinct=…: {} bytes; generation=…; verification=…")` inside the regular test `thousand_row_ranges_are_complete_and_overflow_fails_closed`. Leftover measurement output. DELETE 8 lines. (The `eprintln!` at 968-971 belongs to the ignored experiment, see 2.1.) **safe**
3. Functions too long, with the split:
   - blocks.rs:277-628 (352 lines): see 2.3.
   - range_proofs.rs:701-841 `inspected_range_connects_exact_boundary_nodes_postings_and_rows` (clippy: 132 lines, complexity 34) holds four independent checks on one 5-row fixture: trace structure (715-775), three caps (776-791), the binary search for the smallest verification budget that inspection must not fit in (792-816), empty and reversed ranges (817-840). Four tests + `fn five_rows() -> (Engine, ..)`.
   - range_proofs.rs:442-595 (clippy: 149 lines) and prefix_free_terms.rs:510-642 (125 lines; an equality half 517-573 and a range half 574-641 that share nothing but the fixture): both shrink to ~50 lines with 1.3 c/d; split the prefix_free one in two.
   - eq_proofs.rs:672-788 `independent_witness` (117 lines): `rows_and_keys(..)`, `catalog_leaf(..)`, then assembly; see 5.2 for its parameter.
   - codec.rs:402-530 `type_tags_and_values` (122 lines): `type_tag_ids` (403-442), `value_vectors` (444-498), `non_canonical_values_are_rejected` (500-529).
   - prefix_free_terms.rs:649-760 (106 lines): becomes ~45 with 1.4.
   All **safe** (no assertion changes).
4. eq_proofs.rs:292-294, 309-311, 793-795, 931-933, 1001-1003 - `let VerifiedEq::Page(page) = verify(..).unwrap() else { panic!() };` five times with an empty panic (150-152 has a message). One `fn page(request: &EqRequest, previous: Option<&VerifiedContinuation>, bytes: &[u8]) -> <page type>` with a message. -12 lines. **safe**
5. range_proofs.rs:306-335 - `limits` is built with seven `limits.push(RangeProofLimits { x, ..default })` statements (30 lines); the same file uses an array literal for the same purpose at 776-789, eq_proofs.rs at 494-511. `for caps in [ .. ]`. -8 lines. **safe**
6. namespaces.rs:34-38 defines `CREATE_COST`, `TRANSFER_COST`, `DIRECTORY_CHARGE`, `MISSING_KEY_COST`, `EMPTY_SCAN_COST`, and the same file also writes the literals: `applied(53)` at 912, 1074, 1075, 1080; `applied(23)` at 629; `rejected(1, ..)` at 535, 622-626, 1077, 1081. blocks.rs and rollback.rs use literals only, with the derivation in a comment. Pick one style; the literal + "= 1 + 50 + 2" comment is the clearer one, so drop the five consts. **safe**
7. Clones that exist only to print the operation in a panic message: namespaces.rs:201-205 (`apply`), rollback.rs:118-122 (`apply`), 125-130 (`reject`), 248 + 279 (`assert_rejection_is_atomic`) take `op: WriteOp` by value and call `op.clone()` on every execution (clippy `needless_pass_by_value`). blocks.rs:168-173 does it without the clone (`let description = format!("{op:?}")`); or drop `{op:?}` from the message. prefix_free_terms.rs:109 `query_ids(snapshot, term: Term)` clones the term once per page: take `&Term`. Plain redundant clones (clippy): codec.rs:983, eq_proofs.rs:391, 568, ordered_ranges.rs:351, prefix_free_terms.rs:668 (two), range_proofs.rs:523, rollback.rs:760. **safe**
8. `arkiv_db::X` written out in bodies of files that already have a `use arkiv_db::{..}` list - 51 occurrences (`grep -v '^use ' f | grep -c 'arkiv_db::[A-Za-z]'`: rollback 12, host_state 8, namespaces 6, eq_proofs 6, blocks 5, range_proofs 4, ordered_ranges 4, golem_types 3, codec 2, roots 1). Examples: host_state.rs:342 `arkiv_db::SnapshotId::GENESIS`, 650 and 655 `arkiv_db::Term`, 677 and 680 `arkiv_db::Cost`, 751 and 755 `arkiv_db::ArgumentFault`, 773 `arkiv_db::RecordId`; ordered_ranges.rs:267, 269, 348 and range_proofs.rs:267 `arkiv_db::Limits::DEFAULT_INDEXED_LEN` although `Limits` is imported; golem_types.rs:244, 263, 281 and ordered_ranges.rs:87 `arkiv_db::Branch`; eq_proofs.rs:68, 1155, 1209 `arkiv_db::proof::verify_eq_inspected`. Import them. **safe**
9. `engine.snapshot(engine.head()).unwrap()` where `engine.head_snapshot()` exists and is used in the same file: range_proofs.rs x10 (126, 198, 228, 258, 284, 449, 708, 851, 867, 944; `head_snapshot()` at 625, 684, 1008), prefix_free_terms.rs x5 (215, 334, 472, 516, 702). **safe**
10. eq_proofs.rs:612-618 - `for at in [3, 17, 26, 27] { if at < expected.len() { .. } }`: `expected` is a 28-element array literal (583-586), the guard is always true. Drop the `if`. **safe**
11. eq_proofs.rs and range_proofs.rs have no blank line between top-level items (eq: 18-31 `const`/`fn ns`/`fn id`/`fn name`/`fn key`/`fn request`, 47/48, 53/54, 77/78, 86/87, 131/132, 142/143, 176/177, and the `//!` doc at 1-2 glued to `#[path]`; range: 16/17, 65/66, 81/82, 87/88, 99/100, 381/382, 393/394) and use one-letter names (`n`, `s`, `d`, `e`, `p`, `r`, `w`). The other nine files do not. range_proofs.rs:829 `matches!(result,VerifiedRange::Complete(ref r) if ..)` is unformatted (macro body); 468-476 the closure's last expression lacks `;`. **safe**
12. roots.rs:58-60 - `fn schedule(id: u32, weight: u64) -> PriceSchedule { PriceSchedule::uniform(id, weight) }` is a rename of a constructor, used four times (522, 769, 917, 1023). Inline. **safe**
13. Items declared mid-function: blocks.rs:333 (`type Step<'a>` with a `///` doc comment), ordered_ranges.rs:392 (`type Make`). host_state.rs:257 `let mut engine = engine;` (declare it `mut` at 213). **safe**

No `dbg!`/`println!` in the chunk; the only `eprintln!`s are the two in range_proofs.rs above. No `#[allow(..)]` in any of the eleven files.

### II.4 [S2] arkiv-db src: durable, proofs, small modules

*(paths relative to `crates/arkiv-db/src/`)*

Largest line payoff needs an owner decision: S4 (145), S5 (70), S3 (60). Free and certain: S1, S2, S6, S13.
Largest clarity payoff: S7 to S10.

#### S1. Five of six `#[allow(clippy::..)]` attributes hide nothing (**safe**)
There is no `clippy.toml` for arkiv-db (the only one, `crates/arkiv-node/clippy.toml`, sets `disallowed-methods`),
so the default thresholds apply: `too_many_arguments` fires above 7 parameters, `result_large_err` at 128 bytes.
- proof.rs:482 on `proven`: 7 parameters. Dead.
- proof.rs:886 on `generate`: 7 parameters. Dead.
- proof/range.rs:643 on `generate_range`: 7 parameters. Dead.
- host.rs:174 `too_many_arguments` on `try_expire_records_observed`: 7 parameters. Dead.
  Evidence that 7 does not fire: `terms_inner` (proof/range.rs:326-334) has 7 parameters and no `allow`, and
  `clippy-uniq.txt` contains no "too many arguments" line at all.
- host.rs:150 and host.rs:174 `result_large_err`: both functions return `Result<Result<.., ExpiryHalt>, StorageError>`.
  The lint looks at the outer `Err` type, `StorageError { kind, detail: String }`
  (`crates/arkiv-db-storage/src/versioned.rs:581-586`), about 32 bytes. Dead by reading; I could not run clippy to confirm.
- host.rs:123 on `expire_records` (`Result<Vec<..>, ExpiryHalt>`): probably live. `ExpiryHalt` is a `Vec` plus
  `Rejected`, and functions returning `Result<_, Rejected>` carry no `allow` and produce no warning, so
  `Rejected` sits just under 128 bytes and `ExpiryHalt` just over.

Fix: delete the five; change host.rs:123 to `#[expect(clippy::result_large_err, reason = "..")]` (MSRV 1.90 has
`expect`). An `expect` that stops firing is reported by the compiler, so this cannot rot again.

#### S2. `PreparationStats::emitted_by_map` is written on every staged object and read nowhere (**safe**)
`rg -n emitted_by_map crates conformance scripts docs` returns two lines: the field (durable/prepare.rs:19) and
the write (prepare.rs:189, a `BTreeMap` entry update per object on the commit path). The only purpose of
`Staging::map` (prepare.rs:109), the `kind: MapKind` parameter of `Staging::simple` (prepare.rs:177, 181) and the
assignments at prepare.rs:134, 146, 153, 158 is to feed it. Fix: delete the field, `Staging::map`, the `kind`
parameter and the five `MapKind` arguments at prepare.rs:142-144, 151, 161. -14 lines. It is a `pub` field of a
`pub` struct, so this is an API removal with no user.

#### S3. The import-checkpoint API has no production caller (**behaviour-affecting**, owner decision)
`rg -n "prepare_import|import_checkpoint|initialize_from_staged" crates conformance scripts` finds callers only in
`crates/arkiv-db/tests/streaming_import.rs`, `crates/arkiv-db/tests/adopt.rs` and
`crates/arkiv-db-redb/tests/streaming_import.rs`. arkiv-node's own test name calls the mechanism legacy
(`crates/arkiv-node/src/genesis_streaming.rs:1585` `.._legacy_checkpoint_is_never_selected`).
Affected code: `checkpoint_key` (streaming.rs:11-13), `prepare_import` (236-262), `import_checkpoint` (263-269),
`initialize_from_staged` (270-279), the carry-over at streaming.rs:365-369, and the `checkpoint` parameter of
`prepare_inner` (mod.rs:786, 856-862). About 60 lines plus their tests if it is retired; if it is kept, say in
the docs who is meant to call it.

#### S4. `BlockMap` and `HostError` are a self-described test harness in the public API (**needs-care**, owner decision)
host.rs:244: "This is a test-harness mapping, not consensus evidence". Importers
(`rg -l -U "arkiv_db::(\{[^;]*\bBlockMap\b|BlockMap)" crates conformance scripts`): `examples/records.rs`,
`examples/blocks.rs`, `tests/blocks.rs`, `tests/history.rs`, `tests/pagination.rs`, `tests/query_history.rs`.
Nothing in arkiv-node or conformance. All five `HostError` variants are produced only by `BlockMap` methods.
That is host.rs:30-72 and 236-336 (about 145 lines) plus the two unit tests. Options: move to
`tests/support` and inline a ten-line version in the two examples, or keep and drop "test-harness" from the doc.

#### S5. The `Database` KV facade has no user outside its own tests (owner decision)
lib.rs:119-178 (60 lines) and the re-exports `MemoryStore`, `Entry`, `KvStore`, `WriteBatch`, `WriteOperation`
(lib.rs:89-93). Importers of `arkiv_db::Database`: `tests/semantics.rs`, `tests/backend.rs`, `tests/model.rs`,
`tests/kv.rs`, `examples/basic.rs`. `KvStore` and `MemoryStore`: `tests/semantics.rs`, `tests/backend.rs`.
`WriteOperation`: `tests/backend.rs` only. The engine "shares nothing with the raw facade" (lib.rs:22-23). If it
stays, fix its doc (C1); if it goes, lib.rs loses about 70 lines and its four test files and `examples/basic.rs` go with it.

#### S6. Exported but unused (**safe**)
- `STATE_ROOT_DOMAIN` (lib.rs:116 re-export, state.rs:35): `rg -n -w STATE_ROOT_DOMAIN crates conformance scripts`
  hits only state.rs:35, state.rs:64 and lib.rs:116. Drop the re-export; make the const private.
- `MapKind::ALL` (state.rs:163-173): `rg -n "MapKind::ALL" crates conformance` is empty. Its doc says it defines
  the order of `CacheStats::hits_by_map`, yet cache.rs sizes those arrays with a bare `7` four times
  (cache.rs:85, 86, 260, 261). Either use it (`[u64; MapKind::ALL.len()]`) or delete it. Using it is better.
- `try_expire_records_observed` is the only expiry entry point production calls
  (`crates/arkiv-node/src/exec.rs`) and the only one of the three not re-exported at lib.rs:103.
  `try_expire_records` is imported by tests only. Re-export the one that is used.
- Wider than needed: `pub(super) fn verify_staged` (adopt.rs:456) has one caller, adopt.rs:588, in the same file
  and only forwards to `verify_split(.., SPLIT_ENTRIES)`: inline it (-9 lines). `pub(crate) fn decode_error`
  (format.rs:279) is used only in format.rs. `pub const PROFILE` and `GENESIS_BYTES` (format.rs:10, 15) are used
  only in format.rs. `pub const METADATA_MAX_BYTES`, `INCLUSION_MAX_BYTES`, `MAX_INCLUSIONS`
  (simulator.rs:8, 11, 12) are used only in simulator.rs.

#### S7. Files with no blank lines between items (**safe**)
rustfmt does not insert them. Blank lines per file: durable/bulk.rs 4 of 922, explore.rs 2 of 162,
durable/simulator.rs 8 of 406. Items that start on the line after a closing `}`: bulk.rs 21, proof.rs 25,
proof/range.rs 12, durable/mod.rs 11, simulator.rs 5, streaming.rs 5
(`awk 'prev ~ /^}$/ && $0 ~ /^(pub |impl|fn |struct |enum |const |type |#\[|\/\/\/)/ {print FILENAME":"FNR} {prev=$0}' <file>`).
Worst runs: bulk.rs:16-22, 54-58, 87-99, 116-134, 147-173; proof.rs:77-78, 101-102, 110-111, 130-131, 253-254,
259-260, 310-311, 317-318, 329-330, 345-346, 352-353, 436-437, 446-447, 481-482, 519-520.

#### S8. `bulk.rs` routes every error through a helper called `io` (**behaviour-affecting**: message text only)
`fn io(e) -> DurableError { incompatible(format!("bulk scratch: {e}")) }` (bulk.rs:16-18) has 25 `map_err(io)`
uses on real I/O and 23 uses on string literals that are not I/O: "builder previously failed", "duplicate host
key", "host count cap exceeded", "bulk genesis association", "bulk target already published" (full list:
`rg -n 'io\("' crates/arkiv-db/src/durable/bulk.rs`). Each surfaces as `Incompatible` with the prefix
"bulk scratch: ", for example "bulk scratch: bulk genesis association". Fix: keep `io` for the 25; use
`incompatible("bulk: ..")` for validation, and `corrupt(..)` for the run-integrity ones (bulk.rs:75, 82, 138,
527, 609). No test matches these texts.
bulk.rs:390 also has a typo: `"bulk requires namespace1 and sort buffer4096..=1GiB"`, and one message covers two
unrelated preconditions (bulk.rs:388).

#### S9. Tuple-index soup in `simulator.rs` decoders (**safe**)
simulator.rs:68-82, 237-261 and 379-404 decode into an anonymous tuple and then test `decoded.0` .. `decoded.4`
(for example `decoded.3.is_empty() || decoded.4 != 0`). Fix: destructure at the binding,
`let (height, id, root, payload, rest) = ..`. The three `map_err(|_| corrupt("malformed simulator .."))` at
78, 245 and 388 also discard the `DecodeError`; `format::decode_error` (format.rs:279) keeps it.

#### S10. Hand-computed sizes with no name and no derivation
- format.rs:15 `256` is `<PriceSchedule as Decode>::MIN_ENCODED_LEN` (codec.rs:1386: `4 + 4 + 8 * 31`), and
  format.rs:18 `348` is `StateHeader::MIN_ENCODED_LEN` (state.rs:84-85). Adding an `OperationClass` changes the
  real sizes and leaves these stale. Fix: `GENESIS_BYTES = 8 + 6 * 4 + 32 + Limits::MIN_ENCODED_LEN + <PriceSchedule as Decode>::MIN_ENCODED_LEN + 8 + 1`
  and `MANIFEST_MAX_BYTES = <EngineManifest as Decode>::MIN_ENCODED_LEN + 40 + 32`. **safe [CONSENSUS on-disk]**: same values today.
- proof verifier path depths: `17` (proof.rs:641, 718, 719, 752; proof/range.rs:482, 553, 554, 601), `67`
  (proof.rs:765; proof/range.rs:619), `65` (proof.rs:1081). They are `2 * K::MAX_LEN + 1` for 8-byte ids
  (layout.rs:94, 111), 33-byte record keys (layout.rs:130) and 32-byte host keys. The generator derives the same
  bound from `K::MAX_LEN` (proof.rs:828, 842); the verifier hard-codes it at 11 sites. Fix:
  `const fn path_nodes<K: TrieKey>() -> usize { 2 * K::MAX_LEN + 1 }`. **safe [CONSENSUS]**: same values today.
- `limits.max_cells.min(64)` at proof.rs:387, 530, 993 and proof/range.rs:813; `max_payload_len().max(32)` at
  proof.rs:391, 997 and proof/range.rs:817. Name the 64 and the 32 once.
- prepare.rs:32-85: upper bounds on leaf encodings written as `4`, `5`, `4 + .. + 2`, `8`, `1`, `40` and
  `208 + name.len()`. Nothing says what 208 is. Fix: a named const beside each encoder in layout.rs, or one
  comment per literal. **needs-care**: a wrong bound admits an oversize object or rejects a valid one.
- simulator.rs:10 `BLOCK_FRAMING_BYTES = 116`, simulator.rs:15 `8 + 8 + 32 + 1 + 61 + 4 + 32`, simulator.rs:137
  `i.key.len() + 8 + 8 + 32 + 1 + i.key.len() + 4 + i.value.len() + 32`: three hand-kept copies of the framing
  written by `append` (simulator.rs:149-172). Fix: `const MAGIC: usize = 8; const DIGEST: usize = 32;` etc.
- mod.rs:64: `[(self.cache_entries, 128usize), (self.prepare_objects, 128)]`, a tuple array whose second
  element is always 128. Fix: `const SLOT_BYTES: usize = 128;` and iterate the two counts.
- proof.rs:112 `MIN_ENCODED_LEN = 108` and proof.rs:140 `152`, where the neighbours are written as sums.

#### S11. Long functions with an obvious split
- bulk.rs:494 `finish`, 131 lines: `fn build_postings(sorter, sink, callback) -> Result<(B256, u64), _>`
  (519-554) and `fn build_host(sorter, sink, callback)` (576-616). `finish` keeps the sequencing.
- mod.rs:455 `initialize_inner`, 111 lines: falls under 60 after D2.
- mod.rs:782 `prepare_inner`, 103 lines: `fn check_candidate(&self, candidate, packet) -> Result<u64, DurableError>`
  for the guards at 788-825.
- explore.rs:39 `explore_tree`, 115 lines with a nested `fn view` declared after a statement:
  `fn locate(..) -> Result<Option<ExploredMap>, StorageError>` (67-101) and
  `fn decode_leaf(kind, key, value) -> Result<Option<ExploredLeaf>, StorageError>` (106-158).
- proof.rs:606, proof.rs:887, proof/range.rs:453, proof/range.rs:644: see D1.

#### S12. Arguments taken by value and only borrowed (clippy `needless_pass_by_value`)
- mod.rs:461 `candidate: Option<SealedCandidate>` in the private `initialize_inner`: take `Option<&SealedCandidate>`. **safe**.
- mod.rs:766 `packet: Option<simulator::SimulatorPacket>` in the private `prepare_packet`: take `Option<&..>`. **safe**.
- proof.rs:890 and proof/range.rs:647 `header: StateHeader` (`pub(crate)`): take `&StateHeader`; the callers at
  engine.rs:772 and 791 become `&self.state_header()`. **safe**.
- adopt.rs:517-520 `genesis`, `options`, `identity`, `packet` in the public `adopt_staged_simulator`.
  **needs-care**: public signature, called from `crates/arkiv-node/src/signed/host.rs`; the sibling
  `initialize_*` functions really consume theirs, so by-value may be kept for symmetry.

#### S13. Needless clones and containers (**safe**)
- proof/range.rs:387 `range: range.clone()`: `range` is not used again on that branch; move it.
- proof/range.rs:735 `BTreeMap<RecordId, ()>` (`insert(id, ()).is_some()` at 773, `.keys()` at 781): a `BTreeSet`.
- reachability.rs:241 `head_manifest.clone()` to get a value that is then only borrowed (245).
- prepare.rs:135 `options: options.clone()` into `Staging`: borrow `&'a Options`.
- proof/range.rs:402 calls `query.tag()?` once per returned term; hoist it above the `map`.
- proof/range.rs:315-324 `terms` only forwards to `terms_inner(.., None)` and has one caller (727).

#### S14. Glob imports and module wiring
- `use super::*;` in six non-test modules: adopt.rs:18, bulk.rs:2, simulator.rs:4, streaming.rs:2, explore.rs:2,
  proof/range.rs:7. range.rs pulls about 60 names from proof.rs this way, including private helpers. The clippy
  output lists the explicit import set for each. **safe**.
- explore.rs lives at `src/explore.rs` but is mounted as `engine::explore` through
  `#[path = "explore.rs"] pub mod explore;` (engine.rs:83-84) so that it can glob-import engine's privates.
  Move the file to `src/engine/explore.rs` and drop `#[path]`. **safe**.

#### S15. Stringly typed where a type exists (**needs-care**: one caller in arkiv-node)
explore.rs:41 `kind: &str` is matched against `"catalog"`, `"records"`, `"keys"`, `"rows"`, `"terms"`,
`"postings"` in two separate `match` blocks (67-100 and 106-158). `MapKind` (state.rs:145-160) has exactly those
variants plus `Host`. An unknown string returns `Ok(None)`, the same answer as "path not found". Fix: take
`MapKind` (reject `Host`), and parse the string at the HTTP boundary
(`crates/arkiv-node/src/signed/state_api.rs:80`). Same function: `.map_err(|e| corrupt(e.to_string()))` eight
times (90, 108, 123, 124, 138, 143, 148, 151), and single-letter bindings `m`, `n`, `l`, `t`, `p`, `r`
(50, 71, 74, 123, 124, 147).

#### S16. `Display` that prints the Rust variant name
- mod.rs:99: `other => write!(f, "durable lifecycle: {other:?}")` yields "durable lifecycle: WriterUnavailable".
- proof.rs:215-219: `impl Display for ProofError` is `write!(f, "{self:?}")`.
- bulk.rs:431: `io(format!("invalid record: {e:?}"))`.
Fix: one short sentence per variant. **behaviour-affecting** (text only). arkiv-node maps `ProofError` by
variant (`crates/arkiv-node/src/simulator/mod.rs:46-48`, `demo/runtime.rs:616-617`), not through `Display`, so
no response body I found depends on this text.

#### S17. Naming left over from an earlier shape (**safe**)
bulk.rs: values of type `Run` are called `path`, `paths`, `p` and `out` (224, 229-239, 243, 249, 276-281,
285-297), for example `fn insert_run(&mut self, mut level: usize, mut path: Run)`. Rename to `run` / `runs`.

### II.4 [S4] arkiv-db-redb tests and examples

*(paths relative to `crates/arkiv-db-redb/`)*

#### 4.1 `#![allow(dead_code)]` (tests/common/mod.rs:8, tests/engine_support/mod.rs:1)  (emphasis a)
What is actually dead, per binary (from the imports of each file):

| binary | uses from `common` |
| --- | --- |
| engine_crashes | **nothing** - `mod common;` at tests/engine_crashes.rs:5 is unused; delete the line (`rg -n common crates/arkiv-db-redb/tests/engine_crashes.rs` -> line 5 only) |
| engine_history, engine_process | `BenchDir` only |
| streaming_import, contract | `BenchDir`, `keccak` (+ `crash_store` in contract) |
| recovery, write_amplification | `BenchDir`, one `measure` |
| backends, history, durability, bounded_reads, latching | `BenchDir`, `block`, `commit` (+ `assert_applied`, `crash_store`) |
| isolation, initialization, disk_full, reconciliation, atomicity, relabel | the above + their one submodule (relabel: 4 items of `crash`) |
| examples: engine_storage_experiment | `BenchDir`, `filesystem_of`, `base_dir` |
| examples: reachability_report, durable_throughput | `BenchDir` |

So each of the 18 test binaries compiles all 1,579 lines and uses at most one of the seven submodules
(1,330 lines); `filesystem_of` and `mb` are dead in every test binary (examples only);
`assert_absent` is imported by one test file (reconciliation.rs). For `engine_support`: engine_crashes uses everything,
engine_process leaves only `disk_store` (38-41) unused; the rest of the dead code is in the three
examples (durable_throughput uses 7 of 21 items).

Things the blanket allow currently hides (never read by any test or example; verified with
`rg -n '\.quick_repair|probe_commits|\.objects_per_block' crates/arkiv-db-redb/tests crates/arkiv-db-redb/examples`):
- `RecoveryRow::quick_repair` (tests/common/recovery.rs:14, set at 111) - the example prints its loop
  variable instead (redb_experiment.rs:304).
- `RecoveryRow::probe_commits` (recovery.rs:37, set at 119).
- `Series::objects_per_block` (tests/common/amplification.rs:14, set at 84) - same, redb_experiment.rs:592-596.
  DELETE the three fields (they only show up in `{:?}` output). **safe**. 9 lines.

Would one integration-test crate remove the need? Mostly:
- `tests/it/main.rs` with `mod common; mod engine_support; mod atomicity; ...` (move the 18 files and the
  two support dirs under `tests/it/`): `engine_support` needs no allow at all; `common` is compiled and
  linked once instead of 18 times; the 18 `mod common;` lines go.
- Still dead in that crate, because only examples/redb_experiment.rs reads them: `filesystem_of`, `mb`
  (move both next to `ms`/`machine` in the example), and report-only fields
  `Tally::{crash_points, images, indeterminate_outcomes, calls_per_commit, syncs_per_commit}`,
  `Phase::{tables, cold, warm}`, `RecoveryRow::{blocks, app_backend_at_open, commit_p50}`,
  `Series::{elapsed, p50, p99}`, `Sweep::{quotas, base, needed, committed}`. Five struct-level
  `#[allow(dead_code)] // read by examples/redb_experiment.rs` replace the file-level one.
- The examples keep needing an allow because they `#[path]`-include a tree they use 1-3 items of; put it
  where it is true: `#[allow(dead_code)] #[path = ".."] mod common;` at the four include sites. Splitting
  `BenchDir` + `base_dir` (mod.rs:30-71) into `tests/it/common/bench_dir.rs` lets the three engine
  examples include 42 lines instead of 1,579.
- What must be touched with the move (**needs-care**): tests/engine_process.rs:120
  `.args(["--exact", "engine_process_child", ..])` becomes `engine_process::engine_process_child`;
  `#[path]` in tests/engine_history.rs:5 and in four examples; the `--test atomicity` commands at
  atomicity.rs:70/164/172; docs/engine-storage-v1-report.md:197-198
  (`--test engine_crashes --test engine_process --test engine_history`).
- Alternative, owner's call: `arkiv_db_redb::testing` is already an unconditional public module
  (src/lib.rs:76) holding `CrashDisk` and `SplitMix`; moving `BenchDir`/`block`/`commit`/`assert_applied`/
  `crash_store` there removes `#[path]` and the allow for everything in mod.rs, at the price of fixture
  code in the library.

#### 4.2 Leftover line that exists to silence an unused import
tests/backends.rs:106 `let _: Option<BackendError> = None;` is the only use of `BackendError` imported at
line 21. DELETE line 106 and the import. **safe**.

#### 4.3 tests/common/isolation.rs: a `retain` whose result is thrown away
Lines 282-284 keep only `Manifests` and `Inclusions` records of the history block; the very next call
`chain.commit` (line 286) runs line 100 `batch.records.retain(|(table, _, _)| *table == Table::Meta);`,
which drops them all (no caller ever adds a `Meta` record, so line 100 is `batch.records.clear()`), then
adds `Blocks` + `BlockIndex`. So the history phase writes the same records as every other phase and the
comment at 271 ("touching B, the catalog and records") overstates it. Fix: delete 282-284, write line 100
as `batch.records.clear();`; or, if manifests/inclusions were meant to grow, exempt them in `Chain::commit`
(**behaviour-affecting** for the measurement). Deleting is **safe**. Goes away entirely with 1.7.

#### 4.4 Needless `Arc` in tests/contract.rs
Line 37 `let dir = Arc::new(BenchDir::new("contract"));` is never cloned; it is moved into the `reopen`
closure and referenced once (`let _keep = &dir;`, line 43). A plain `BenchDir` does the same. Drop `Arc::new`
and `use std::sync::Arc` (line 18). **safe**.

#### 4.5 Redundant clones (clippy), beyond 1.2
tests/latching.rs:141 `Health::Failed(first.clone())`, tests/reconciliation.rs:153
`CrashDisk::from_image(image.clone())`, examples/durable_throughput.rs:125 `store.clone()`. **safe**.

#### 4.6 Misnamed / misleading names
- examples/bench_readers.rs:286, 305: `commit_ms: Vec<u32>` stores `as_micros()`; lines 347-348 divide by
  1000 to print ms. Rename `commit_us`. **safe**.
- examples/key_size_report.rs:283 `range_terms` holds `proof.supplied_nodes` (printed as
  `range_supplied_nodes_mean`, line 329). Rename `range_nodes`. **safe**.
- examples/reachability_report.rs:106-111 `let _dir;` is assigned and then used (`_dir.file(..)`)
  (clippy: used underscore-prefixed binding). Rename `demo_dir`. **safe**.
- tests/engine_crashes.rs:22 `fn open(image, large)` also begins a branch, applies the candidate and
  prepares it. Rename `prepared(image, large)`. **safe**.
- tests/atomicity.rs:111 `head_is_parent_until_the_commit_call_returns_into_its_final_phase`: the body and
  its comment (112-114) test monotone publication. Rename `publication_is_monotone_over_crash_points`. **safe**.
- Single-letter parameters only in the engine files: tests/engine_process.rs:40-70 (`s`, `t`, `k`, `b`),
  tests/streaming_import.rs:10-12 (`i`, `e`), engine_support/mod.rs:19 (`s`).

#### 4.7 Three files are in a different, squashed style
tests/engine_support/mod.rs (17 items start on the line after the previous `}`), tests/streaming_import.rs
(no module doc; `mod common;` / `use` / `fn` / `#[test]` with no blank lines at 1-2, 9-10, 15-16, 38-39),
tests/engine_crashes.rs:21-22 and 115-116, tests/engine_process.rs:38-39 and 50-51,
examples/engine_storage_experiment.rs:22-58 (6). rustfmt does not add blank lines, so they pass
`fmt --check` but read unlike the other 15 test files.
`rg -cU '^\}\n(pub )?(fn|impl|struct|const) ' crates/arkiv-db-redb/tests crates/arkiv-db-redb/examples`.
Fix: blank line between items. **safe**.

#### 4.8 Inline fully-qualified paths next to an import of the same module
tests/common/disk_full.rs:12-13 (`arkiv_db_redb::RedbConfig` twice; line 4 imports from the crate),
tests/history.rs:28 (`arkiv_db_storage::versioned::Head`; line 25 imports the module),
tests/engine_crashes.rs:22 (`arkiv_db_redb::RedbStore`), 214 and 217
(`arkiv_db_storage::versioned::StorageErrorKind::Full`), tests/engine_support/mod.rs:181, 190
(`arkiv_db::Projection::ALL`), tests/backends.rs:45 and tests/common/recovery.rs:40 (`std::path::Path`),
examples/reachability_report.rs:45, examples/key_size_report.rs:249-251, examples/durable_throughput.rs:48, 67
(`arkiv_db::Branch`). Import them. **safe**.

#### 4.9 tests/engine_crashes.rs:212-220 two arms with the same body
`Publication::NotCommitted(ref e) if !during_commit => assert_eq!(e.kind, Full)` and
`Publication::Indeterminate(ref e) if during_commit => assert_eq!(e.kind, Full)`. Match on the pair instead:
`let error = match (&outcome, during_commit) { (Publication::NotCommitted(e), false) | (Publication::Indeterminate(e), true) => e, other => panic!(..) };`
then one `assert_eq!(error.kind, StorageErrorKind::Full);`. **safe**.

#### 4.10 Magic numbers repeated between builder and oracle (tests/engine_support/mod.rs)
`candidate` 102-103 (`192`/`4` values, `9000`/`256` bytes) and `check` 212, 218 repeat them; `check` 176
(`17`/`16` rows) mirrors `base` 90 + `candidate` 101; 162 is `assert!(seq == 1 || seq == 2)`.
Fix: `const fn candidate_shape(large: bool) -> (u64, usize)` used by both; `const BASE_ROWS: u64 = 16;`. **safe**.

#### 4.11 Long functions with an obvious split
- examples/bench_readers.rs:182-360 `main` (179 lines; clippy 165/100): `create_namespace(&mut engine) -> NamespaceId`
  (203-224), `preload` (234-245), `spawn_readers` (247-283), `run_producer` (285-310), `report` (318-355).
- examples/key_size_report.rs:166-332 `main` (167): `parse_args`, `bootstrap` (182-217), `write_records`
  (219-241), `proof_sizes` (255-304), report.
- examples/engine_storage_experiment.rs: `history` 60-194 (135), `historical_proofs` 196-327 (132),
  `live_growth` 359-505 (147): each interleaves measuring, `format!` rows and prose.
- tests/common/isolation.rs:187-299 `run` (113; clippy 103/100): four phases that each do
  open / commit loop / drop / `phases.push(measure(..))`; a `grow(&path, &mut chain, |store, chain| ..)`
  closure wrapper removes the repeated open/drop/measure (lines 221+233-241, 246+260-268, 273+288-296).
**needs-care** (examples have no tests).

#### 4.12 Prose and results hard-coded as string literals
examples/engine_storage_experiment.rs has 20 lines over 150 characters, all `println!` literals
(`awk 'length > 150' ..`: 161, 164, 170, 174 (803), 177, 183, 186, 202, 205, 292 (597), 295, 320 (1,080),
376, 379, 488, 491, 497 (538), 500, 520, 531 (1,634)). Line 531 states conclusions as fixed text
("Application cold open performed eight record reads ...") that must be kept equal by hand to the
assertion at line 144 (`assert_eq!(delta.record_reads, 8)`); the checked-in report already says "seven"
(docs/engine-storage-v1-report.md:110). Fix: interpolate measured values (`{record_reads}`), and move the
static paragraphs to `examples/engine_storage_report/*.md` printed with `include_str!` so they wrap and diff
as text. Other files: redb_experiment.rs 9 such lines (239, 275, 288, 291, 447, 454, 491, 510, 516),
reachability_report.rs:129, durable_throughput.rs:181. **safe** for output-identical moves.
Same hazard in small: redb_experiment.rs:203, 208, 213 hard-code "2 + 128 torn modes" / "2 + 16"
(derived from `HEAVY_SEEDS`/`SEEDS`), and 264-267 hard-code "50" beside `for height in 0..50`.

#### 4.13 Benchmarks that default to tmpfs
examples/bench_readers.rs:54 and examples/key_size_report.rs:167 default `--dir` to
`std::env::temp_dir()`, which tests/common/mod.rs:3-6 and docs/testing.md:367-369 say must never be used
("a RAM tmpfs here, where `fsync` does nothing"). scripts/bench-compare.sh:617 always passes `--dir`, so
only manual runs are affected. Fix: make `--dir` required, or default to `ARKIV_BENCH_DIR`. **behaviour-affecting**
(defaults only).

#### 4.14 Smaller items
- examples/redb_experiment.rs:206 `let (tally_off, _) = timed(|| crash::run(false, crash::SEEDS));` - the
  duration is discarded; call `crash::run` directly. **safe**.
- examples/key_size_report.rs:298 `if let Ok(proof) = snapshot.prove_range(..)` drops every error silently;
  the reported `range_proofs=` just shrinks. Print the error to stderr or `?` it. **behaviour-affecting**.
- examples/key_size_report.rs:80-139 `workload: &str` is matched per record and an unknown name panics at
  139, after the store has been created; parse into `enum Workload` at 176. **safe**.
- examples/bench_readers.rs:224 and key_size_report.rs:217 ignore the `Publication` returned by
  `engine.commit()` for the namespace block while `commit_block`/`commit` check it.
- tests/engine_crashes.rs:102, 169 use `eprintln!`; the other seven test files that print tallies use
  `println!`. Pick one.
- tests/common/disk_full.rs:70 `let candidate: CommitBatch = block(..)` needless annotation.
- tests/write_amplification.rs:36-37 and 51-52: the same two sync-bound assertions; a
  `fn assert_sync_bounds(&Series)` is optional.
- tests/common/recovery.rs:86-93 averages `BackendStats` field by field.
- clippy `format_push_string` at reachability_report.rs:138, 162, 173.

### II.4 [S5] arkiv-db-trie

*(paths relative to `crates/arkiv-db-trie/`)*

**4.1 `src/explore.rs` is a style outlier (120 lines)**
Reads as written by a different hand than the rest of the crate:
- no blank lines between the module doc, imports and items (`:1-2, :8-9, :15-16, :27-28`); undocumented pub fields
  on `ExploreChild`/`ExploreNode` while every other pub field in the crate is documented;
- one-letter bindings `s` (:36), `n`, `b` (:54-57), `c` (:64, :71, :84), `i` (:71), `p` (:105);
- magic `16` for "extension step" at `:36, :64, :100, :105` -> `const EXTENSION_STEP: u8 = 16;`;
- `crate::TrieError::inconsistent` fully qualified twice (`:79, :114`) instead of importing `TrieError`;
- the trailing `Ok(None)` at `:118` is unreachable (the `for depth in 0..=path.len()` loop always returns at
  `depth == path.len()`, `:47`). Natural shape: `for &step in path { descend }` then describe the node after the
  loop, which also un-nests the 50-line `if depth == path.len()` block (`:47-98`);
- `:54-58` re-encodes and re-hashes an in-memory child although `NodeBox::reference()` caches exactly that;
- see also 1.13 (own key-length checks) and 1.16 (`nibbles::pack`).
**safe** except the loop restructure (**needs-care**; `tests/explore.rs` and arkiv-db's explorer cover it).

**4.2 Functions too long, with the split**
- `src/interval.rs:364-580` `walk_interval_inner` (209 lines): extract (a) node fetch `:409-438` (1.11),
  (b) trace node `:444-487` (1.10), (c) decision recording (1.15); the remaining match is ~110 lines.
- `src/proof.rs:316-466` `walk_inner` (144 lines): besides 1.10/1.11, the terminal is recorded in four separate blocks (five terminal values) as
  `if let Some(trace) = trace.as_deref_mut() { trace.terminal = X; } return finish(index, ..)` (`:398-405, 412-415,
  424-427, 431-434`). Let the loop `break (terminal, value)` and set `trace.terminal` / call `finish` once after
  it. -12 lines. **needs-care [CONSENSUS]** (verifier; pure control-flow refactor).
- `tests/store.rs:68-247` `store_and_reopen` (157 lines, cognitive complexity 39): split at its own comment
  boundaries into `assert_staged_is_reference_set` (76-84), `assert_reads_match_memory` (86-145),
  `apply_lazy_updates` (147-189), `assert_restaged_and_history` (191-246). **safe**.

**4.3 Unused pub items (rg over crates/ conformance/ scripts/, excluding this crate)**
- `MemoryNodeStore::ids` (`src/loader.rs:136`): `rg -n '\.ids\(\)'` -> no use anywhere, tests included. DELETE.
- `CodecError::reason` (`src/codec.rs:22`): `rg -n '\.reason\(\)'` -> no use anywhere. DELETE (Display covers it).
- `KeyRange::all` (`src/interval.rs:53`): `rg -n 'KeyRange::all|::all\(\)'` -> no use anywhere. DELETE.
- `NodeBox::reference` (`src/node.rs:184`) is `pub` and returns `&alloy_trie::nodes::RlpNode`, a vendored type the
  crate does not re-export; `rg -n '\.reference\(\)'` -> only `src/` of this crate. Make it `pub(crate)`
  (`Link::reference` already is).
- `order::KeyWriter` / `order::KeyReader` (`src/order.rs:160-266`, ~107 lines) are used only by this crate's
  `tests/encoding.rs:158,167`, the doctest and one unit test; arkiv-db builds its keys with the free functions
  (`crates/arkiv-db/src/layout.rs:299-311`). Either adopt them in layout.rs or delete them and write the test
  `Term` codec with `encode_bytes`/`decode_bytes`. **needs-care**.
- Test-only pub surface, fine to keep but worth knowing: `MemoryNodeStore` (zero uses outside this crate; it is
  the documented test double and the lib.rs doctest needs it), `order::ByteKey` (only `tests/variable_keys.rs`,
  `tests/sorted.rs`, docs), `MerkleMap::root_link` (tests only), `KeyRange::closed` (tests only),
  `StageBudget::remaining` (one assertion, `tests/bounded_stage.rs:212`).
All **safe** (`publish = false`).

**4.4 `#[allow]`**
- Six `#[allow(dead_code)]` on the shims (1.1); one of them hides a really dead fn.
- `tests/support/mod.rs:15` `#![allow(dead_code)]`: standard for a shared test module; fine.
- `tests/bounded_stage.rs:59` `#[allow(clippy::ptr_arg)]` has a correct why-comment; fine.

**4.5 Formatting rustfmt gave up on**
- `tests/bounded_stage.rs:430-439`: closure with an explicit return type split over nine lines
  (`&mut |_,\n _|\n -> Result<\n usize,\n StageError,\n > {`). Bind it first:
  `let mut never = |_: &Vec<u8>, _: &mut StageBudget| -> Result<usize, StageError> { panic!("stored leaf callback") };`. **safe**.
- No line over 120 chars in the chunk.

**4.6 Magic 32 / 33**
- The inline threshold appears as a bare literal in non-test code at `src/node.rs:211, 436`, `src/proof.rs:452`,
  `src/interval.rs:417`, `src/explore.rs:57`, `src/stage.rs:182`, and the hash-reference length as `33` at
  `src/stage.rs:147, 182` (`Link::Empty => Ok(1)` at :146 likewise). Two consts in node.rs
  (`INLINE_LIMIT = 32`, `HASH_REFERENCE_LEN = 33`) would tie them together. **safe**.

**4.7 Needless clones flagged by clippy (tests only)**
- `src/node.rs:677` `leaf.clone()`, `:729` `leaf.clone()`, `:813` `path.clone()`: last uses; drop `.clone()`.
- `tests/map.rs:136`, `tests/store.rs:452`, `tests/variable_keys.rs:206`: same.
- `tests/interval_model.rs:51` `check(model: BTreeMap<..>, bounds: Vec<[u8; N]>)` takes both by value without
  consuming them; take `&BTreeMap` / `&[[u8; N]]` and drop the four `.to_vec()` at `:117, :120, :123`.

**4.8 Inconsistencies**
- `src/interval.rs:171` generator state is a 4-tuple `Vec<(Link<V>, Role, usize, Option<u8>)>` while the verifier
  in the same file has the named `PendingNode` (`:355-362`); give the generator a named struct too.
- `IntervalProofIter::new` takes `max_key_len` and doubles it (`interval.rs:177-181`); `ProofIter::new` takes
  `max_nibbles` (`proof.rs:62`); `map.rs:464` vs `map.rs:482` pass different units.
- `usize::from(nibble)` everywhere in map.rs/range.rs/proof.rs, but `as usize` at `src/sorted.rs:187, 247`.
- `crate::NodeKind` spelled out four times in `src/proof.rs:234, 356, 363, 370` and three in `src/interval.rs:451-460`
  instead of an import; `arkiv_db_trie::ProofError::InvalidNode` / `arkiv_db_trie::Link` likewise in
  `tests/store.rs:741, 900, 919, 946`.
- Missing blank line between items at `src/proof.rs:215-216`; no blank lines between trait methods at
  `tests/variable_keys.rs:520-527` and `tests/bounded_stage.rs:30-45, 93-103, 219-234`.

**4.9 Clippy `into_iter` on one item (tests, node.rs)**
- `src/node.rs:687` -> `RlpNode::word_rlp(&B256::repeat_byte(0x11)).to_vec()` (both already imported in the test module);
  `:711`, `:717` -> `std::iter::once(..)`. The production encoder (`node.rs:259-328`) has nothing to fix beyond 1.16.

### II.4 [T2] arkiv-db tests: history / isolation

*(paths relative to `crates/arkiv-db/tests/`; `h` = history.rs, `qh` = query_history.rs, `i` = isolation.rs, `qi` = query_isolation.rs)*

Functions too long, with the split:
- query_isolation.rs:1514-2493 `expected_baseline` (980 lines, clippy 854/100). Split along its own sub-banners:
  `expected_point_reads(&Model)` 1534-1574, `expected_cursor_walk(&Model)` 1576-1642,
  `expected_operator_queries(&Model)` 1644-1806, `expected_misses()` 1808-1823, `expected_counts(&Model)` 1825-1860,
  `expected_aborted_reads()` 1862-2079 (needs no model), `expected_writes(&mut Model)` 2081-2407 (itself
  creates 2081-2177 / patches 2178-2264 / deletes 2265-2335 / expiry 2336-2407),
  `expected_reads_after_writes(&Model)` 2409-2491. The first six are the read workload, so `READ_STEPS = 39`
  (813) becomes their summed length instead of a literal. **safe**
- query_history.rs:3343-3742 `run_seed` (400 lines, clippy 363) and history.rs:1513-1856 (344, clippy 321).
  The phases are already marked by comments and `Ctx` rebuilds: `check_genesis`, `execute_block`
  (qh 3406-3473 / h 1593-1688), `expire_block` (qh 3488-3542), `seal_and_stage` (qh 3544-3568 / h 1690-1713),
  `publish_block` (qh 3583-3610 / h 1730-1789), `check_applied` (qh 3612-3732 / h 1791-1838; in qh further
  `check_expired_rows` 3634-3674, `check_typed_sharing` 3676-3697, `check_stale_cursor` 3699-3722). **needs-care**
- query_history.rs:2853-3039 `expire_checked` (187): oracle half `predict_expiry(..)` 2886-2987, engine half 2988-3038.
- query_history.rs:2649-2800 `execute_checked` (152): `Coverage::observe(..)` for 2688-2733, `check_branch_after_write(..)` for 2753-2796.
- query_history.rs:2036-2208 `check_reads` (173): per-namespace `check_stats` 2047-2062, `check_get_records` 2064-2097, `check_catalogue_entry` 2099-2205.
- query_isolation.rs:3109-3350 test: see category 2.

`#[allow(clippy::too_many_arguments)]` x8 (history.rs:1219, 1285, 1403; query_history.rs:1907, 2648, 2802, 2852, 3110):
- query_history.rs:2802 is unnecessary: `rollback_checked` has 7 parameters and the lint fires above 7. Copied from history.rs:1285 where the function has 8. DELETE. **safe**
- The others hide a real smell: `transcript` and `coverage` always travel together, and `Run` (h:1506, qh:3293) is exactly that pair but is only assembled at the end of `run_seed`. Create `Run` first and pass `&mut Run`; pass `(actor/path, op, budget)` as the tuple `plan_op` already returns; give `Ctx` a `phase()` method instead of passing `seed` and `block` separately. All eight allows go. **needs-care**

Other:
- query_history.rs:3522-3539 - 18 lines re-derive the key of every expired id (previous retained view, else a scan of the pending log) although `expire_checked` had `(id, record_key)` in hand at 2915. Return `Vec<(RecordId, RecordKey)>` from `expire_checked` (2862, 3036) and delete the lookup. **safe**
- query_isolation.rs:1094 `if step.starts_with("count")` decides between `count` and `query` from the step's display label, and the three count rows carry a dummy projection (1089-1091). Likewise 1123 `if step == ABORT_EQ_PAGE { 4 } else { MAX_PAGE }` and query_history.rs:2171 `*label == "match-all"`. Use a separate `counts` array, a `limit` column, and a flag or index. **safe**
- query_isolation.rs:186-207 `TermLedger` identifies terms by `format!("{name}:{:?}", cell.value)` and `format!("{name}:{:?}", cell.value.tag())` in a `BTreeSet<String>`: identity through `Debug` output. query_history.rs already has a typed key (`TermId` 496-499, and `(CellName, tag, Option<OrdValue>)` in `term_count` 651-663); use that. **safe**
- history.rs:777-795 `verdict_name` is an exhaustive 15-arm match that includes nine rejections this file cannot produce and must be edited for every new `Rejection` variant; query_history.rs:1418-1425 does the same job with `format!("{reason:?}")`. Share qh's version (add a `StaleRevision { .. }` arm). **safe**
- query_history.rs:1122 `enum Path { Owner(OwnerId), Host }`, documented as "Which authority a write runs under", shadows the usual meaning of `Path`. Rename to `Authority`. **safe**
- query_history.rs:2583-2598 - arms `0 | 1` and `3` are identical, as are `2` and `6` (clippy "identical bodies" 2584, 2589). Write `0 | 1 | 3` and `2 | 6`, keeping the comment at 2588. **safe**
- query_isolation.rs:98 `const MAX_PAGE: usize = 200;` commented "`Limits::DEFAULT.max_page`": use the constant (`Limits::DEFAULT` is usable in const context, see query_history.rs:94). **safe**
- isolation.rs:61 `const A_RECORDS: u64 = 64;` is used three times, always as `A_RECORDS as usize` (978, 1062, 1276). Declare it `usize`. **safe**
- query_isolation.rs:1524 `let miss = |spent, lines| read_receipt(spent, lines);` is an alias used once (1814). DELETE. **safe**
- query_isolation.rs:2297 redundant `.clone()` on the last use of `out_of_budget` (clippy). **safe**
- history.rs:1225 and query_history.rs:2653 take `op: WriteOp` by value and clone it for `execute` (clippy "passed by value, but not consumed"); take `&WriteOp`. **safe**
- history.rs:1989-1999 hand-rolled index loop plus length check where query_history.rs:3874-3877 uses one `assert_eq!` on the vectors. **safe**
- No `println!`/`dbg!`, no lines over 150 characters (one of 101 at query_history.rs:2882), no dead items.

### II.4 [T1] arkiv-db tests: metering family

*(paths relative to `crates/arkiv-db/tests/`; `qm` = `query_metering.rs`)*

#### 4.1 Over-long test fns with an obvious split

Spans measured `fn` line to closing brace. Clippy flagged the ones marked (c).

| file:lines | len | fn | split |
|---|---|---|---|
| qm:2703-2915 (c) | 213 | `range_queries_visit_only_the_qualifying_terms` | runs that yield rows (2710-2816, 2882-2914) / the three empty runs -> seek-only table (section 2) |
| qm:3325-3511 (c) | 187 | `or_groups_open_in_order_and_merge_interleaved_heads` | two-group OR with limit 2 and continuation (3327-3432) / three interleaved groups (3433-3471) / empty group ORed with a posting (3472-3510) |
| qm:3035-3210 (c) | 176 | `negated_first_literal_and_match_all_walk_the_row_map` | negated first literal (3037-3096) / match-all paging and continuation (3097-3209) |
| qm:2426-2600 (c) | 175 | `eq_query_pages_charge_the_head_beyond_a_full_page` | page 1 + cursor continuation + ledger (2429-2486, 2574-2599) / single-page variants (2488-2539) / two seek-only vectors -> table |
| qm:3866-4022 (c) | 157 | `execute_host_receipts_are_identical_to_execute_for_the_same_op` | parity loop (3867-3927) / host skips the owner check (3929-3959) / raw and namespace ops refused at admission (3961-4021). Three independent branches already |
| qm:4433-4535 | 103 | `cost_overflow_inside_a_group_charges_nothing_of_the_group` | two unrelated engines: `CellByte` at `u64::MAX` (4434-4495) / `IndexTermCreate` at `u64::MAX` (4497-4534, re-binds `engine`, `ns`, `branch` by shadowing) |
| qm:2067-2173 | 107 | `delete_charges_the_removal_group_then_one_unit_per_held_term` | 2153-2172 ("deleting every record empties the index, a recreated key gets a fresh id") is a different rule; own test |
| qm:1959-2060 | 102 | `patch_that_would_overfill_the_record_is_record_full_after_the_lookup` | 2031-2059 (`TooManyCells` admission fault, spent 0) is a different rule; own test |
| qm:1339-1450 (c) | 112 | `create_and_get_record_under_a_32_byte_key_count_the_raw_key_bytes` | create (1342-1395) / get_record (1397-1449) |
| bounds.rs:189-355 (c) | 167 | `index_values_have_a_hard_byte_cap_...` | writes refused at admission (220-259) / queries and `terms` refused on branch and snapshot (261-319) / the at-limit value is stored and queryable (320-354) |
| bounds.rs:32-101 | 70 | `names_are_bounded_and_follow_namespace_history_and_transfers` | three behaviours behind "and": name validation (33-43) / rollback frees the ordinal and names are not unique (44-75) / transfer keeps the name and shares records with the old snapshot (77-100) |
| trace_regression.rs:756-834 | 79 | `pulls_failing_at_posting_entries_probes_and_row_visits_keep_the_source` | NotEq page (759-800) / match-all page (802-833); no shared state beyond the fixture |
| scalar_profile.rs:121-230 (c) | 110 | `all_scalars_prove_equality_and_reject_typed_substitution` | no split needed after 1.2 item 7 |

Long but fine as they are (one sequential scenario or one data table): metering.rs:843-985 and qm:3701-3859 (block-budget scripts, each step depends on the remaining budget), metering.rs:1300-1454 and qm:4029-4223 (script tables), metering.rs:1841-1952 and qm:4542-4697 (pricing before/after publication), qm:1017-1140, 2274-2388, 3518-3647 (vector tables), query_planner.rs:659-764.

#### 4.2 Other

1. query_planner.rs:797-800 - leftover `eprintln!("clustered count: ...")` in `intersections_count_without_loading_rows`. **DELETE** (-4). Only debug output in the chunk (`rg -n 'eprintln!|println!|dbg!'` over the seven files). **safe**.
2. Redundant clones on a last use (clippy, each checked against the following lines): qm:1088 `b.clone()` inside `setup: &[a.clone(), b.clone()]`; qm:1636, 1672, 1736, 1800, 1906, 2096 `let mut expected = model.clone();` where `model` is not used again (write `let mut expected = model;`); cell_limits.rs:164 `ok.clone()`, 178 `long.clone()`; query_planner.rs:616 `lang.clone()`, 617 `target.clone()`. **safe**.
3. Path-qualified names inside files that already import from the crate, the mark of a field added by search-and-replace: `arkiv_db::NamespaceName::new("test")` 18 times in metering.rs and at qm:646, 675, 3966; `arkiv_db::Revision(0)` qm:3970; `arkiv_db::NamespaceName` and `arkiv_db::Branch` query_planner.rs:243, 254; `arkiv_db::Branch` cell_limits.rs:36, 52; `arkiv_db::Limits::DEFAULT_INDEXED_LEN` bounds.rs:203, 224, 266, 330 although the file has `use arkiv_db::*;`. Import and shorten. **safe**.
4. scalar_profile.rs - four tests carry their own `use arkiv_db::{..}` blocks (122-125, 234, 250, 313) and there is no blank line between the module doc and the first `use` (1-2). bounds.rs:4 is the only `use arkiv_db::*;` in the chunk. Hoist and list imports like the other files. **safe**.
5. qm:4732 - `page.value.rows.iter().map(|row| row.id.ordinal()).collect()` is `ids_of(&page.value.rows)` (qm:623). Same expression at query_planner.rs:483 and 910. **safe**.
6. query_planner.rs:597-610 - `target` is found by scanning `live`, then `live` is scanned again by that address to fetch `lang` from the same record. One `find(..).map(|cells| (cells[&name("addr")].value.clone(), cells[&name("lang")].value.clone()))`. -6 lines. **safe**.
7. query_planner.rs:718-727 - distinct values counted by `sort_by_key(|value| format!("{value:?}"))` + `dedup()` (a `String` allocation per comparison, ordering by Debug text). `Value: Hash + Eq` (`src/record.rs:518`), so `.collect::<HashSet<_>>().len()`. **safe**.
8. scalar_profile.rs:218 - `(arkiv_db::proof::PROOF_VERSION ^ 3).to_be_bytes()` to turn 2 into 1. Line 241 of the same file writes `1u32.to_be_bytes()` for the same purpose. **safe**.
9. scalar_profile.rs:306 - `assert_eq!(result.is_ok(), accepted);` in an 11-row loop with no message: a failure does not say which value. Add `"{value:?}"` (bind the value before moving it), or see the MERGE in section 2. **safe**.
10. qm:683-692 - `fixture_with` asserts the receipts `379 / 229 / 257` while building the fixture, so a change to create pricing fails all 35 tests that call `fixture()` with the same message instead of only `create_vectors_on_an_empty_namespace_match_the_worked_examples` (1017), which pins the same three numbers under every budget. Replace with `.expect("fixture creates apply")` only. **needs-care** `[CONSENSUS]` (pin survives at 1048/1075/1102).
11. qm:3690-3692 - `spent` and `kind` asserted separately because one of the three reads was traced (3683) for no stated reason. Drop `.traced()` and assert `rejected.receipt == read_receipt(0, OUT_OF_BUDGET)`. **safe**.
12. query_planner.rs:236-323 - the 3,000-record fixture (plus ~130 deletes and ~90 patches) is rebuilt by each of the 7 tests. A `static FIXTURE: LazyLock<Fixture>` would build it once. Not verified: whether `Engine` is `Sync` (`src/engine.rs:438`, field `nodes: Nodes` not checked). **needs-care**.

### II.4 [NT3] arkiv-node tests: genesis / simulator / demo / CLI; examples; Cargo.toml; clippy.toml

*(paths relative to `crates/arkiv-node/`)*

**S1. `examples/verify_inspection.rs:114-185` - a 72-line `#[cfg(test)]` test that CI never runs.** needs-care
Cargo's default for example targets is `test = false`; there is no `[[example]]` section in `Cargo.toml` and CI runs `cargo test --offline --locked --workspace` (`.github/workflows/ci.yml:49`), which only compiles examples. `rg -n '\-\-examples|\[\[example\]\]'` over the repo: no hit. (It is the only `#[cfg(test)]` in any `crates/*/examples`.) I did not run cargo; this rests on Cargo's documented target defaults.
Fix (pick one): add `[[example]] name = "verify_inspection"` / `test = true` to `Cargo.toml` (3 lines, safe), or move `verify()` (48-112) into the library next to `simulator::inspection` and the test into `tests/simulator.rs`, leaving the example as argument parsing (better: the docs present it as a verification tool).

**S2. `clippy.toml` - all 31 listed methods still exist, two infallible forms are missing.** needs-care
Checked every entry against `crates/arkiv-db/src/engine.rs` / `host.rs`: 16 `Branch::*`, 14 `Snapshot::*`, `host::expire_records` all present (script: every `pub fn try_X` vs its un-prefixed twin gives exactly these 31).
Missing: `arkiv_db::engine::Branch::checkpoint` (`engine.rs:1963`, twin `try_checkpoint` :1972) and `arkiv_db::engine::Branch::seal` (`engine.rs:2313`, twin `try_seal` :2322). Both doc comments say they panic on a poisoned branch, "which only an engine that reads storage can cause; such a host calls `try_...`" - exactly the case the header of `clippy.toml` describes.
`rg -n '\.checkpoint\(\)|\.seal\(\)' crates/arkiv-node` -> non-test call sites `src/exec.rs:960`, `src/arkiv/mod.rs:839`, `src/genesis_bundle.rs:578`; tests `src/arkiv/tests.rs:1267,1319,1377`, `tests/exec.rs:217,266`.
Fix: add the two entries (`reason = "use try_checkpoint"` / `"use try_seal"`) and convert the eight call sites (`src/exec.rs:680` already uses `try_seal`). Not listed and correctly so: `Snapshot::state_root` / `state_header` ("never reads storage, so it cannot fail", `engine.rs:841-858`).

**S3. `Cargo.toml` dependencies.**
- Line 33 `alloy-primitives.workspace = true` under `[dev-dependencies]` duplicates line 10 (`alloy-primitives = { workspace = true, features = ["k256"] }`) and adds nothing. DELETE lines 32-33. safe.
- Line 34 `alloy-trie.workspace = true` looks unused (`rg -n 'alloy_trie' crates/arkiv-node` -> no hit) but is **needed**: `tests/support/block_roots.rs:6-7` `#[path]`-includes `crates/arkiv-db-trie/tests/support/mod.rs`, whose line 20 is `use alloy_trie::{EMPTY_ROOT_HASH, HashBuilder, Nibbles, proof::ProofRetainer};`. Keep it and say so in a comment, otherwise the next cleanup removes it.
- Every other dependency is used (rg per crate name): `alloy-rlp`, `alloy-sol-types` (`src/signed/ethereum.rs`, `rpc.rs`), `k256` (`src/signed/crypto.rs:4`), `arkiv-db-trie` (`simulator/inspection.rs`, `signed/state_api.rs`, `block.rs`), `ed25519-dalek` (demo only, see (c)), `toml` / `sha2` / `base64` (`genesis_authoring.rs` only), `rustix` (`genesis_prebuilt.rs`, `genesis_import_resources.rs`), `getrandom` (the three bins), `axum` `ws` (`signed/rpc_ws.rs`), `reqwest`, `tokio`, `serde`, `serde_json`. The `k256` feature of alloy-primitives is needed (`crypto.rs:64` `recover_address_from_prehash`).
- Four exact pins without a comment: `alloy-sol-types = "=1.7.3"` (12), `k256 = "=0.13.4"` (13), `ed25519-dalek = "=2.2.0"` (18), `toml = "=0.8.23"` (21); the workspace's `redb` pin has one (`Cargo.toml:30` at the root). `toml` has a real reason that should be written down: `toml::to_string(&profile)` is hashed into genesis locks as `expanded_profile_sha256` (`src/genesis_authoring.rs:1011,1017`) and re-checked at `:1185-1190`, so a serializer change invalidates existing locks `[CONSENSUS]`. For the other three the only rationale is `docs/signed-proposer-v1-plan.md:78-96` ("already present in this repository's lockfile"); with `Cargo.lock` committed and CI `--locked`, either add a one-line reason each or relax to caret requirements. Adding comments: safe; relaxing: needs-care.
- `tokio` features (26) lack `io-util`, yet `tests/simulator_http.rs:25`, `tests/signed_http.rs:37`, `tests/signed_reads.rs:62` import `tokio::io::{AsyncReadExt, AsyncWriteExt}` (io-util only). It builds through feature unification with axum/reqwest. Add `tokio = { version = "1", features = ["io-util"] }` to `[dev-dependencies]`. safe.

**S4. Temp directories leak on failure and are named ad hoc.** safe (folded into D1's `TempDir`)
`genesis_prefill.rs:617-621` (removed only on success at :651), `simulator_http.rs:102-104` / :122 and `:427-432` / :520 (the second name uses `free_address().replace(':', "-")` - a throw-away port probe - as its uniqueness token), `retention_cli.rs:7-12`.
`retention_cli.rs:67-68` cleans up with `remove_dir_all(run_dir("sim-light"))`: `run_dir` itself deletes the directory and returns the path, so the helper is called for its side effect and the directory is removed twice (the first `dir` binding was shadowed at :53).

**S5. Redundant clones (clippy-uniq, all last uses).** safe
`tests/demo_auth.rs:181` (`valid.clone()`), `:216` (`trust.clone()`, `key.clone()`); `tests/simulator.rs:222` (`store.clone()`), `:224` (`config.clone()`), `:453` (`key.clone()`).

**S6. Lines rustfmt gave up on (macro bodies / raw strings).** safe - break the literal by hand, then rustfmt formats the rest
> 150 chars: `examples/gas_calibration.rs:118` (420), `examples/verify_inspection.rs:110` (351), `tests/simulator_http.rs:400` (251), `examples/verify_inspection.rs:144` (184; the whole statement is unformatted: `let selection:VerifiedRequest=serde_json::from_value(...)`), `examples/verify_inspection.rs:84` (164), `tests/demo_auth.rs:251` (162), `tests/simulator_http.rs:281` (161), `tests/genesis_adopt.rs:425` (151).
110-150: `tests/simulator_http.rs:152` (150), `examples/verify_inspection.rs:105` (150), `tests/simulator.rs:366` (142), `examples/verify_inspection.rs:37` (141), `tests/demo_auth.rs:107` (140), `tests/genesis_adopt.rs:500` (135), `tests/simulator_http.rs:56` (116).
Unspaced macro bodies: `tests/simulator.rs:328` `matches!(Light::open(store,&h.identity),Err(e) if e.0=="ChainConflict")` and `:509` `matches!(c,Change::RecordDelete{record_id,..} if record_id=="1")`; `tests/simulator_http.rs:689` `format!("0x{}","00".repeat(1024*1024+1))`. Finder: `awk 'length > 110 {print FILENAME":"FNR": "length}'`.

**S7. Function-local `use` and fully-qualified paths instead of file-level imports.** safe
Local `use` blocks: `genesis_prefill.rs:592,663`; `simulator.rs:391-393` (re-imports `Value`, already imported at :3); `simulator_http.rs:525-528,712`; `demo_auth.rs:264-266,399`; `examples/gas_calibration.rs:9-20` (all imports inside `main`).
Qualified paths in bodies (`rg -c '^\s+.*\b(arkiv_node|arkiv_db)::'`): genesis_prefill 14, simulator 9, demo_auth 9, simulator_http 7, genesis_adopt 4. E.g. `genesis_prefill.rs:596` writes `arkiv_node::genesis_bundle::ROW_CODEC` although `genesis_bundle::*` is glob-imported at :11; `arkiv_node::exec::total_key` four times (389, 678, 684, 822); `arkiv_node::TxStatus::Committed` :323 vs local `use ... TxStatus` at :663.

**S8. `tests/retention_cli.rs:86,120`** `let _child = Child(..)` then `drop(_child)` (clippy "used underscore-prefixed binding"). Name it `child`. safe.

**S9. `tests/simulator_crash.rs:142-146`** `eprintln!("simulator redb cuts=...")` in a test; the coverage it reports is already asserted at :141 (`parents > 0 && candidates > 0`). DELETE, or assert the counts. safe. (Same habit elsewhere: `crates/arkiv-db-redb/tests/engine_crashes.rs`, `tests/signed_reads.rs` - not this chunk.)

**S10. `examples/gas_calibration.rs`** - the whole program is one `main` (8-122; clippy too-many-lines 111/100) with its imports inside the function (9-20), a `const` after statements (:22), and the 420-char `json!` at :118. Split: file-level imports + `const PERIOD_MS`, `fn quote(label, body, days, attributes, clock) -> Result<serde_json::Value, _>` (36-118), `main` = table + print. Mixed error style in a `-> Result` main: `.unwrap()` at :23, :53, :109, :111, `.expect` :101, `panic!` :98 next to `?`; `:53` can be `??` (`Rejected: Error`, `engine.rs:1503`), `:23` `.ok_or("rent clock")?`. safe (output must stay byte-identical: `docs/glm-gas-v1-calibration.json` is this example's output; its keys match the `json!` at :118).

**S11. `examples/verify_inspection.rs:48-53`** `verify(response: Value, genesis: GenesisResponse, header: HeaderDto, selection: VerifiedRequest)` consumes none of its arguments (clippy, 4 warnings); taking references removes the `genesis.clone(), header.clone()` per call in the test closure (:160-161) and five `.clone()`s at :163-180. safe.

**S12. No blank line between top-level items** (rustfmt-clean, but reads as generated). Items glued to the previous item: `genesis_prefill.rs` 18 (e.g. 43, 50, 69, 72, 91, 107, 149, 270, 327, 487, 497, 516, 568, 572, 588, 653, 657), `simulator_http.rs` 12 (31-81, 316, 366-396), `demo_auth.rs` 6 (24, 32, 58, 117, 213, 237), `genesis_adopt.rs` 4 fns (60, 63, 148, 410) plus impl blocks, `prebuilt_genesis_cli.rs` 4 fns (44, 47, 106, 151) plus impl blocks, `simulator_crash.rs` 4 (17 follows the `use` block directly), `simulator.rs` 3, `examples/verify_inspection.rs` 4. The same files separate other items with blank lines, so it is also inconsistent within each file. safe.

**S13. Magic numbers.** `genesis_adopt.rs:215` `3` (the `threads` argument of `Host::adopt_staged`; the producer passes `genesis_adopt::verify_threads()`); `genesis_prefill.rs:172` `6_307_201` (see Comments); `simulator_http.rs:587` `1024 * 1024` and `:584` `4096` caps repeated from `examples/verify_inspection.rs:69,88`. safe.

**S14. Marker scan off by one.** `genesis_adopt.rs:625` and `prebuilt_genesis_cli.rs:324`: `for at in 0..bytes.len() - MARKER.len()` skips a match ending at the last byte; use `bytes.windows(MARKER.len())` positions (in the shared `flip_marker`). safe.

**S15. `prebuilt_genesis_cli.rs:353-354`** `free_port().to_string()` then `format!("127.0.0.1:{port}")`; `genesis_adopt.rs:654` does it in one `format!`. safe.

**(d) Examples: referenced and current.** `inspect_eq` - `docs/simulator-proof-inspection.md:162-164`; `verify_inspection` - same doc :180; `gas_calibration` - `docs/glm-gas-v1.md:348`. All three have entries in the clippy run, i.e. they compile against today's API. Not stale themselves; what is stale is the doc around them: the table at `docs/simulator-proof-inspection.md:150-157` (posting counts 5 / 1 / 2 / 1 for heights 20 / 10-str / 13 / 14) contradicts the fixture pinned by `tests/simulator_http.rs:555-562` (3 / 0 / 4 / 3, "creates five group-1 entities at height 2, deletes one at 8 and expires one at 14"), so the documented `inspect_eq -- 10 str 1 2` no longer demonstrates "string equality is distinct". Also `docs/testing.md:458` names `protocol_two.rs`, which does not exist (`rg protocol_two` -> docs only).

### II.4 [D1] Documentation: stale, redundant and finished-plan documents

*(repo-relative paths. The documentation review uses its own five headings; all of them belong to this category.)*


#### [D1] Summary (read this first)

1. **The README and architecture.md describe an earlier engine.** 6 + 7 direct contradictions with the code (R1-R6, A1-A6, A8): cell-name and
   indexed-value limits (20/64 vs 32/128), padded record keys (unpadded since state layout 2), block format "version 1"/"2" (5),
   state format 1 (2), range-proof caps 64/64 (1,000/1,000), "31 JSON-RPC methods" (32), a crate table that omits the signed
   proposer, simulator, gas and genesis modules, and a "Next increments" section that says durable node production is future work.
2. **docs/testing.md is organised as a build diary** ("node step N0b-1 ... One arithmetic slip was made during the re-derivation")
   and covers 63 of 98 integration-test files; it names a test file and a test function that do not exist.
3. **Plans kept beside their implementations**: 5 `*-plan.md` files (2,500 lines) plus three design-before-code specs
   (engine-storage-v1.md, genesis-bundle-design.md, full-light-demo.md). Two plans say "implemented" and can be deleted, two are
   progress logs wrapped around still-normative tables (charge table, record model), one (base fee) is unimplemented and belongs
   in proposals/; the specs keep their format/proof sections and lose their "implementation sequence" parts.
4. **Run logs and one-host deployment notes in docs/** (about 1,400 lines + 2 JSON blobs + a 39 KB HTML slide deck), with
   `/home/ubuntu/...` paths, `rogue-one` host names and "N tests passed" totals.
5. **Dead-converter doc still advertised**: README.md:419-420 links docs/sourcify-prefill-converter.md, whose own banner says the
   node refuses its output (row codec 2); docs/full-prefill-docker.md tells the reader to run it.
6. **Undefined labels**: N0a..N6, milestone 0-5, D0-D3 appear about 365 times in docs and 51 times in source comments; they are
   defined only inside the plan files proposed for archiving.
7. One code-affecting item: `genesis_authoring.rs:49` compiles a file under `docs/examples/` into the binary as the `@5` genesis
   preset [CONSENSUS]; and `vendor/reth-trie` is a workspace member nothing depends on.

Everything else in this file is the evidence, per file, with a concrete action.

#### [D1] 1. Stale or contradicting docs

#### README.md

| # | Where | Doc says | Code / other doc says | Action | Risk |
|---|---|---|---|---|---|
| R1 | README.md:63-64 | "the canonical key encoding is version 2, 33 bytes `len ‖ bytes ‖ zero padding`" | `crates/arkiv-db/src/record.rs:22-24`: "The canonical encoding ... is `len ‖ bytes`, 2 to 33 bytes"; `docs/block-format.md:132` `u8 len ‖ bytes`, 2-33; CHANGELOG.md:313-320 "Unpadded, prefix-free keys" | FIX: "canonical encoding `len ‖ bytes`, 2 to 33 bytes" | safe |
| R2 | README.md:70 | "Cell names are at most 20 ASCII characters; indexed values are at most 64 bytes" | `record.rs:131` `CellName::MAX_LEN = 32`; `record.rs:868` `Limits::DEFAULT_INDEXED_LEN = 128`, `record.rs:584` `INDEXED_LEN_CEILING = 512`. (20 is the *namespace name* cap, `namespace.rs:26`, and the raw-key cap, `record.rs:863`.) | FIX: "Cell names are at most 32 ASCII bytes; indexed strings at most 128 bytes by default (512 ceiling)" | safe |
| R3 | README.md:167 | range proofs: "initial caps: 64 terms and 64 rows" | `crates/arkiv-db/src/proof/range.rs:204-205` `max_terms: 1_000, max_rows: 1_000`; `docs/range-proofs.md:35-36` "at most 1,000 distinct matching terms and 1,000 matching rows" | FIX to 1,000/1,000 or drop the numbers and link range-proofs.md | safe |
| R4 | README.md:431 | "all 31 JSON-RPC methods" | 32: the 28 `eth_/net_/web3_` arms in `crates/arkiv-node/src/signed/rpc.rs:956-1234` plus `arkiv_query`, `arkiv_getEntity`, `arkiv_getEntityCount`, `arkiv_getBlockTiming`; `crates/arkiv-node/src/signed/state-openapi.json:2773` lists the same 32 under "Supported methods" (`eth_subscribe`/`eth_unsubscribe` are WebSocket-only and extra). `arkiv_getEntity` was added later (CHANGELOG.md:139). | FIX: drop the count ("every JSON-RPC method") so it cannot rot | safe |
| R5 | README.md:240 (crate table, `arkiv-node`) | "The pseudo node's library (node step N0b-3): block format version 1 ... and the bounded producer/full/light executable (`demo`, `arkiv-demo`)" | `crates/arkiv-node/src/block.rs:47` `FORMAT_VERSION = 5`; the crate also holds `signed/` (17 source files), `simulator/`, `arkiv/`, `gas.rs`, `accounts.rs`, `retention.rs`, seven `genesis_*.rs` and three binaries (`src/bin/arkiv-{demo,proposer,simulator}.rs`), none listed | FIX: rewrite the row: block format + executor, the Arkiv entity host (`arkiv`, `gas`, `accounts`), genesis authoring (`genesis_*`), and three runtimes (`demo`, `simulator`, `signed`) with their binaries | safe |
| R6 | README.md:139-140, 154 | "Block format ... (node step N0b-3). The `arkiv-node` library defines block format version 1"; link text/anchor `docs/block-format.md#part-2-block-format-version-1` | `block.rs:47` `FORMAT_VERSION = 5`, `block.rs:58` `PROTOCOL_VERSION = 5`; CHANGELOG.md:178 "block format version 5" | FIX: say "block format (currently version 5)"; rename the block-format.md heading (see BF findings below) | safe |
| R7 | README.md:210-213 | "All history is **retained in memory**, with **no pruning**. Production stops at configured capacity. Every producer restart requires a new genesis ..." (un-scoped paragraph after "Run the demo") | true only of `arkiv-demo`; README.md:186-190 and 215-221 say the simulator and proposer are durable and restart without replay. A reader hits three paragraphs that alternately say volatile / durable / volatile. | MERGE the three paragraphs (186-229) into one "Status" table: runtime x {durable, signed, proofs} | safe |
| R8 | README.md:47-48, 283, 305, 344, 360, 381 | "Milestones 0, 1 and 2 of the implementation plan are now implemented in memory", "The milestone-1 engine files", "The milestone-2 engine files", "The node-step-N0b-2 files", "The node-step-N0b-3 files", "node step N2" | labels are defined only inside `docs/snapshot-query-plan.md` (milestones) and `docs/node-storage-plan.md` (N0a..N5); a README reader cannot resolve them | FIX: replace by what the thing is ("engine tests", "typed-record and query tests", "codec tests", "node tests", "storage tests") | safe |
| R9 | README.md:279-390 (112 lines) | per-test-file descriptions for about 31 of the 98 integration-test files (98 files match `crates/*/tests/*.rs`) | duplicates `docs/testing.md` (570 lines) and already misses 67 files (`adopt.rs`, `bounds.rs`, `cell_limits.rs`, `durable*.rs`, `eq_proofs.rs`, `golem_types.rs`, `query_planner.rs`, `range_proofs.rs`, all `signed_*.rs`, `simulator*.rs`, ...) | DELETE lines 279-390, keep line 392 ("See the testing notes") | safe, -110 lines |
| R10 | README.md:1-229 | 229 lines of dense status prose before the first heading (`## Workspace` at 231); no quickstart for the main binary (`arkiv-proposer`) anywhere in the README | - | RESTRUCTURE: 10-line intro, `## Quickstart` (3 commands from `docs/signed-proposer-v1.md`), `## Status` (table), `## Workspace`, `## Development`, `## Documentation` (index of docs/) | safe, about -150 lines |
| R11 | README.md:183 | "an independent PostgreSQL explorer in the indexer `experimental` checkout" | refers to a checkout on the author's machine, not to anything in this repo | FIX: name the repo/URL or delete the clause | safe |
| R12 | README.md:405 | "Work proceeds in small commits on `main`." | process note; AGENTS.md:6 already says it | DELETE | safe |
| R13 | README.md:253-255 | "The minimum was raised from 1.88 to 1.90 because redb 4.3.0 requires it; `cargo +1.90.0 check --workspace --all-targets --offline` passes." | history + an assertion nobody re-runs; CI (`.github/workflows/ci.yml:15`) already pins 1.90.0 | TIGHTEN to "Rust 1.90 or newer (redb 4.3.0 requires it)." | safe |

Verified OK in README: the Development commands (README.md:258-267) match `.github/workflows/ci.yml:44-50`
and the package/example names (`basic`, `blocks`, `records` in `crates/arkiv-db/examples/`; `redb_experiment`,
`engine_storage_experiment` in `crates/arkiv-db-redb/examples/`; `--quick` at `redb_experiment.rs:130`;
`ARKIV_BENCH_DIR` at `crates/arkiv-db-redb/tests/common/mod.rs:64`; `rehash` test file exists);
`scripts/full-light-demo.py` exists; all five flags named in README.md:23-31 are in the FLAGS list
(`arkiv-proposer.rs:653-657`); `--allocations-file`/`--bundle` at `arkiv-proposer.rs:625,635`; 25 `Value`
variants (`record.rs:519-570`); `MODEL_VERSION = 3` (`meter.rs:27`); `CODEC_VERSION = 1` (`codec.rs:60`);
`HOST_VALUE_MAX = 256` (`engine.rs:128`); `PersistentMap`, `CountingBackend`, `CrashDisk`, `FaultyStore` exist.
Every relative markdown link in README.md and in every docs/*.md resolves (script-checked, 0 broken).

#### docs/architecture.md

| # | Where | Doc says | Code says | Action | Risk |
|---|---|---|---|---|---|
| A1 | architecture.md:126 | "`arkiv-node` (node step N0b-3) is the host above the engine: block format version 2" (and :723 "block format version 1", :138 anchor "...version-1") | `block.rs:47` `FORMAT_VERSION = 5` | FIX: drop the number or say 5 | safe |
| A2 | architecture.md:372 | CellName "... at most 20 bytes" | `record.rs:131` `MAX_LEN = 32`; the same file says 32 at architecture.md:454 | FIX line 372 to 32 | safe |
| A3 | architecture.md:453-455 | "Indexed strings have a hard 128-byte cap (`Value::MAX_INDEXED_LEN`) ... the widths of the term-index key" | `Value::MAX_INDEXED_LEN` no longer exists (`rg MAX_INDEXED_LEN crates` = 0 hits); it is `Limits::DEFAULT_INDEXED_LEN = 128` (policy) and `Value::INDEXED_LEN_CEILING = 512` (`record.rs:584,868`); "no longer a key width" (`record.rs:866`) | FIX: "default 128 bytes (`Limits::DEFAULT_INDEXED_LEN`), ceiling 512 (`Value::INDEXED_LEN_CEILING`); not a key width" | safe |
| A4 | architecture.md:277 | decoding rejects "an attribute of type `Bytes` or over 64 bytes" | `crates/arkiv-db/src/layout.rs:532` rejects `encoded_len() > Value::INDEXED_LEN_CEILING` (512) | FIX "over the indexed ceiling (512 bytes)" | safe |
| A5 | architecture.md:318 and :549-550 | "StateHeader { format u32 = 1, ..." and "the `StateHeader` of state format 1" | `crates/arkiv-db/src/state.rs:32` `STATE_FORMAT = LAYOUT_VERSION` = 2 (`lib.rs:184`); the same doc says "state format 2" at :253 | FIX both to 2 | safe |
| A6 | architecture.md:711 | "model-version-2 metering of every index and query step" | `meter.rs:27` `MODEL_VERSION = 3`; same doc :716 "counting-rule version 3" | FIX or delete with the whole "Next increments" section (A8) | safe |
| A7 | architecture.md:421-423 | "nothing is reordered or planned, so the charged cost and the abort point are a pure function of the state and the query text" | true for the canonical evaluation only; `crates/arkiv-db/src/query.rs:10-20` and `typed/planner.rs` add an opt-in planned evaluation (`Budget::planned`) that README.md:27-31 advertises | FIX: add one sentence naming the opt-in planner (`Budget::planned`) and link its description (today snapshot-query-plan.md:553-566) | safe |
| A8 | architecture.md:692-754 ("Next increments") | "The next work ... is N4 durable node production"; "The current node runtime remains atomic in process and disappears on restart ... durable protocol blocks and producer restart remain N4" | contradicts the same file at :25-29 ("the implemented simulator and signed-proposer runtime have durable storage and restart paths") and README.md:215-221 | DELETE the section (63 lines); it is a progress log in "Done:/Node steps done so far" form. Keep at most a 5-line "Not implemented" list (consensus, physical pruning, broader query proofs) | safe, -58 lines |
| A9 | architecture.md:1 | title "Architecture: KV foundation and immutable snapshots" | the file covers trie state, state root, codec, node host, durable engine | FIX title to "Architecture" | safe |
| A10 | architecture.md:66-69 (crate diagram) and :125-138 | arkiv-node = block / params / validate / select / identity / exec / replay | misses `signed`, `simulator`, `arkiv`, `gas`, `accounts`, `genesis_*`, `retention` (see R5) | FIX: add the three runtimes and the entity host to the diagram | safe |
| A11 | architecture.md:5-6, 20-42 | "The original KV milestone used ...", "Step N2 added ...", "N3c now integrates ...", "N4 references below describe the original milestone plan, not a claim that ..." | history narration; the last sentence is a disclaimer about the doc's own staleness | TIGHTEN "Scope and reference" to the present tense: what exists, link to the two contracts; delete the disclaimer | safe, about -25 lines |

#### docs/testing.md (570 lines, organised by build step, not by subject)

| # | Where | Doc says | Code says | Action | Risk |
|---|---|---|---|---|---|
| T1 | testing.md:84-85 | "`arkiv-node` (node step N0b-3) has eight integration test files in `crates/arkiv-node/tests/`" | 27 files match `crates/arkiv-node/tests/*.rs` | FIX (or drop counts) | safe |
| T2 | testing.md:64-72 | "Integration tests live in `crates/arkiv-db/tests/`: four raw-KV files ... six milestone-1 ... seven milestone-2 ... one N0a ... three N0b-2 ... three N3b" (= 24) | 40 files; 35 of the workspace's 98 integration-test files are not mentioned anywhere in testing.md: all `signed_*.rs` (6), `simulator.rs`, `simulator_crash.rs`, `accounts.rs`, `arkiv_profile.rs`, `ethereum_transactions.rs`, `gas_execution.rs`, `genesis_adopt.rs`, `genesis_prefill.rs`, `prebuilt_genesis_cli.rs`, `retention_cli.rs`; arkiv-db `adopt.rs`, `cell_limits.rs`, `golem_types.rs`, `interrupt.rs`, `ordered_ranges.rs`, `query_planner.rs`, `range_proofs.rs`, `scalar_profile.rs`, `simulator_composition.rs`, `streaming_import.rs`; trie `adversarial_bounds.rs`, `explore.rs`, `interval_*.rs` (3), `sorted.rs`; redb `bounded_reads.rs`, `relabel.rs`, `streaming_import.rs` | Either add one table row per missing file or (better) cut testing.md to the oracle rules + one table "file -> one-line claim" generated from each test file's `//!` header | needs-care (content) |
| T3 | testing.md:458 | "`protocol_two.rs` and `demo_auth.rs`: root timing, strict authentication vectors ..." | `protocol_two.rs` does not exist (`rg -l protocol_two crates scripts` = 0 hits outside this doc) | FIX: delete the name or point at the file that holds those tests now | safe |
| T4 | testing.md:461 | "`block_model.rs::continuous_protocol_two_200_blocks_match_independent_roots_outcomes_and_pages`" | no such function (`rg 'fn continuous_protocol_two' crates` = 0) | FIX/DELETE bullet | safe |
| T5 | testing.md:170, 268-270, 273 | "`RecordKey` encoding version 2: the length-prefixed 33-byte form"; "rejection of length 0, length 33 and non-zero padding"; "not its 33-byte encoding" | `record.rs:22` "`len ‖ bytes`, 2 to 33 bytes"; `crates/arkiv-db/tests/records.rs:648` "`len ‖ bytes`, no padding" | FIX | safe |
| T6 | testing.md:332 | "a row whose attribute holds a `Bytes` value or a 65-byte `Str` is `Corrupt`" | `crates/arkiv-db/tests/storage_faults.rs:804` `let length = Value::INDEXED_LEN_CEILING + 1;` (513) | FIX "a `Str` over the indexed ceiling" | safe |
| T7 | testing.md:564-570 | "`bounds.rs` exercises 20/21-character names ... 64/65-byte indexed strings" | `crates/arkiv-db/tests/bounds.rs:203` uses `Limits::DEFAULT_INDEXED_LEN` (128). (That line's own comment "32 characters, 64 UTF-8 bytes" is stale too: 64 x 2-byte chars = 128 bytes; pass to the bounds.rs reviewer.) | FIX to 128/129 | safe |
| T8 | testing.md:304, 331 | "Step N3b moved every engine map to `arkiv-db-trie` (state format 1)"; "state-format-1 layouts" | `STATE_FORMAT` = 2 (`state.rs:32`, `lib.rs:184`) | FIX | safe |
| T9 | testing.md:443 | "format, protocol and header versions other than 1 ... Block format version 1 is exactly the documented layouts" | `block.rs:47,58` format and protocol version 5 | FIX | safe |
| T10 | testing.md:207-209 | "They do not test proofs, sorting, or a durable backend, because none exist." | `crates/arkiv-db/src/proof.rs:566` `verify_eq`, `proof/range.rs:419` `verify_range`, `crates/arkiv-db/tests/eq_proofs.rs`, `range_proofs.rs`, `durable.rs` exist; the same doc's first section (lines 3-52) describes the durable tests | DELETE sentence | safe |
| T11 | testing.md:491-560 ("Planned proof and availability acceptance tests", "Planned node and durable backend tests") | "Complete query proofs ... Planned (milestone 4)"; "caches and proofs do not exist"; "engine point proofs do not exist"; "the engine and node versions of all five remain a backlog, not currently passing guarantees" | complete equality and range proofs are implemented and tested (`eq_proofs.rs`, `range_proofs.rs`, `prefix_free_terms.rs` - listed in this same doc at :418, :466); decoded-node cache tested (`durable/cache.rs`, this doc :30); engine/node crash tests exist (`engine_crashes.rs`, `engine_process.rs`, `simulator_crash.rs`) | DELETE both "Planned ..." sections (70 lines); keep one short "Not covered" list (snapshot import, wall-clock isolation, physical pruning) | safe, -65 lines |
| T12 | testing.md:91 | CI "runs on every push to `main` and on pull requests" | `.github/workflows/ci.yml:5` `branches: [main, "experimental-*"]` | FIX (minor) | safe |
| T13 | testing.md:318 | "A's 64 raw keys, encoded `key ‖ pad to 20 ‖ len`" | raw keys are `bytes⁰(key)` (escaped, `00`-terminated, architecture.md:262; CHANGELOG.md:320 "Raw-record ... keys are unpadded the same way"). `crates/arkiv-db/tests/isolation.rs:698` carries the same padded wording, so either both are stale or the literal derivation needs re-reading (unverified which) | Check `isolation.rs:698` first, then reword both | needs-care |

Process narration in testing.md (style tell; DELETE, about 60 lines):
- :143-146 "These files were unchanged by milestone 2 apart from the `model_version: 2` literal ..."
- :218-229, :235-240 the N0a bug history ("Before the fix ...", "All seven tests fail against the evaluator before N0a ... Removing only the run half of the fix fails four of them")
- :242-281 "Engine tests: node step N0b-1" - a changelog of which literals were re-derived, ending "One arithmetic slip was made during the re-derivation ... first written as 163 ... redone by hand (161)"
- :309-327 "Re-derived physical literals" table with "was `(1, 10, 15)`", "(was the AVL height bound)"
- :343-353 "Performance: on this machine (32 cores, debug profile, 2026-09-24) ... took 34.1 s before N3b and 40.2 s after"
The oracle rules (:178-205) and the per-file "what it checks / evidence" tables are the valuable part; keep those.

#### Broken anchors (file exists, heading does not) - script-checked over README + every docs/*.md

| Where | Link | Actual heading | Action |
|---|---|---|---|
| README.md:154, docs/architecture.md:138, docs/block-format.md:20, docs/node-storage-plan.md:16, docs/snapshot-query-plan.md:1154, docs/testing.md:428 | `block-format.md#part-2-block-format-version-1` | `docs/block-format.md:396` "## Part 2: block format version 5" (slug `part-2-block-format-version-5`) | Rename the heading to "## Part 2: block format" (slug without a number, so it stops rotting) and fix the six links. safe |
| docs/entity-expiration.md:5, docs/proposals/expiry-spill.md:6 | `block-format.md#expiry` | no heading named "Expiry" at any level in block-format.md | FIX link to `#execution` (block-format.md:655) or add the heading. safe |
| docs/review-storage-2026-09-27.md:63, :83 | `engine-storage-v1-report.md#L170`, `#L149` | GitHub line anchors do not work on rendered markdown | goes away with the review docs (see group G5) |

Dangling backticked paths that look like repo paths but belong to other repos (reader cannot follow them):
`docs/entity-expiration.md:229-230` (`docs/api-spec.md`, `crates/arkiv-reth-payload-builder`), `docs/sdk-1.0-compatibility-todo.md:7,27`
(`docs/api-spec.md`, `docs/sort-implementation-plan.md`), `docs/sourcify-v4-prefill.md:5,134-135` (`docs/udc-proposal.md`,
`docs/data/hydrated-contract.json`, `docs/udc-examples/safeproxy`), `crates/arkiv-node/src/simulator/query_language/README.md:5-6`
(`crates/arkiv-query/src`, `crates/arkiv-interfaces/src/query.rs`). FIX: prefix each with the repo name, e.g. "`arkiv` repo, `crates/arkiv-query/src`".

#### Small READMEs

| # | Where | Finding | Action | Risk |
|---|---|---|---|---|
| V1 | vendor/README.md:3-6 | "The user explicitly authorized copying the Reth trie implementation and adding its dependencies. This supersedes the initial dependency-free restriction ..." - assistant-to-user phrasing in a provenance file | FIX: "The trie sources are vendored deliberately (path dependency, pinned revision); only the KV storage contract and memory backend are standard-library-only." | safe |
| V2 | vendor/README.md:8-27, README.md:11-12 ("The trie layer vendors Reth's generic components"), README.md:241 (crate table row `reth-trie`), README.md:260-261 and `.github/workflows/ci.yml:21` (`-p reth-trie`) | `vendor/reth-trie` has no dependent: `rg 'reth[-_]trie' crates conformance -g Cargo.toml -g '*.rs'` = 0 hits; in Cargo.lock nothing lists it as a dependency; `docs/snapshot-query-plan.md:86` already says "the vendored Reth components are not used" | Owner decision: drop `vendor/reth-trie` from the workspace (then delete the README/CI mentions), or keep and say "unused reference extraction" in the crate table | needs-care (Cargo.lock, CI package list) |
| DP1 | deploy/README.md:34-67 ("Deployment verification (2026-09-29)", "Expanded node reference") | a dated run log for one host (`full-node.rogue-one.eu`), "then run with the since-removed `--features arkiv-v2`", "30 paths / 33 HTTP operations, all 31 HTTP JSON-RPC methods" (now 32, see R4) | MOVE lines 34-67 to `docs/runs/2026-09-29-api-docs-deployment.md` or DELETE; deploy/README.md keeps lines 1-32 (the how-to) | safe, -34 lines |
| QL1 | crates/arkiv-node/src/simulator/query_language/README.md:32-36 | "Valid range, prefix, Boolean and all-entity queries are explicitly classified as unsupported execution/proof shapes." - true for the simulator's proof classification only; the signed node executes them (`arkiv_query` ranges/BETWEEN/ORDER BY, CHANGELOG.md:85-137). Says nothing false, but a reader of this README alone gets the wrong picture | FIX: add "(by the proof classifier; `arkiv_query` on the signed node executes them unverified)" | safe |
| AG1 | AGENTS.md:1-15 | Agent working agreement ("commit and push them to `origin/main` without asking for another confirmation") checked in at the repo root | KEEP if agents keep working here, but it is the first thing that tells a visitor the repo is agent-driven; consider moving to `.github/` or the agent's own config dir | owner decision |

#### Other verified stale lines in otherwise-current docs

| # | Where | Doc says | Code / other doc | Action |
|---|---|---|---|---|
| O1 | docs/block-format.md:13 | "2. **Block format version 3 and block execution** (node step N0b-3 ...)" | same file :396 "Part 2: block format version 5", `block.rs:47` | FIX to 5 (or drop the number) |
| O2 | docs/project-namespaces.md:33 | "Roots, proofs and the authenticated directory remain design targets." | same file :269 "Roots are implemented"; eq proofs verify the directory path (architecture.md:645, `proof.rs:566`) | FIX |
| O3 | docs/project-namespaces.md:164, :269, :337, :347, :350-351 | "Proof material ... do not exist yet"; "proofs are not"; "planned (N3d)"; "durable storage follows in N3c" | `crates/arkiv-db/tests/eq_proofs.rs`, `range_proofs.rs`; `DurableEngine` | FIX status words; delete "Implementation order and acceptance gates" (:304-354) or reduce to the test list without status |
| O4 | docs/project-namespaces.md:271 | "StateHeader { format 1, ..." | 2 | FIX |
| O5 | docs/single-block-query-proofs.md:18, :283-289 | "Implementation has begun entirely in memory"; "The Rust prototype keeps its KV store, snapshots, indexes ... **entirely in memory for now** ... Call this a volatile reference implementation, not durable commits" | `durable::DurableEngine`, README.md:170-177 | DELETE "Implementation stages" (:272-326, 55 lines) |
| O6 | docs/single-block-query-proofs.md:371-380 | "Proof verification uses an independently trusted root ... Planned."; "durable storage are planned"; "In the durable stage, fault injection exposes a complete old or new state ... Planned." | `proof::verify_eq` takes the caller's accepted snapshot (eq-proof-charges.md:5-9); `crates/arkiv-db-redb/tests/engine_crashes.rs`, `engine_process.rs` | FIX the three status words |
| O7 | docs/signed-proposer-v1.md:182-184 | detached reads: "`state/explore`, `query/eq` and `query/range`, and JSON-RPC `arkiv_query` and `arkiv_getEntityCount`" | `state-openapi.json:3222` "only arkiv_query, arkiv_getEntity and arkiv_getEntityCount are detached"; CHANGELOG.md:145 | FIX: add `arkiv_getEntity` |
| O8 | docs/signed-proposer-v1.md:14-17, :115-117 | "The [design plan] records the protocol and dependency decision ... `cargo +1.90.0 check -p arkiv-node --locked` succeeds with the existing MSRV"; "this change does not register a new indexer source or deploy frontend assets" | PR-description sentences ("this change") in a guide | DELETE both |
| O9 | docs/full-light-demo.md:305 | "Derive the exact 164-byte value-term key" | term keys are variable-length since layout 2 (experimental-prekucki-v1.md:90: 164 is only the longest under default limits) | FIX "Derive the value-term key" |
| O10 | docs/pruning-design.md:185-186 | "The checked-in rogue-one files on this machine predate the current state format and cannot be opened." | nothing of the kind is checked in | DELETE sentence |
| O11 | docs/golem-db-conformance-report.md:30-36 | "Every failure is either a golem-internal system record ... or a read of an old commit ..." | `conformance/golem-db/expected-failures.txt:5-8` "All five fail only on golem-internal system records ... The same five fail with the default archive retention and with ARKIV_STATE_RETENTION=0" | FIX the report sentence (low priority) |
| O12 | docs/engine-storage-run.md:11-13 | "The full/light demo still runs in memory; durable node restart is N4." | simulator and proposer are durable hosts (README.md:215-221) | FIX |
| O13 | CHANGELOG.md:254, :275, :289, :294 | `### Entity model`, `### JSON-RPC`, `### Gas`, `### Genesis` at the same level as their parent `### User-defined columns` (:247) | they render as five separate Unreleased entries | FIX to `####` |
| O14 | CHANGELOG.md:302 and :208 | Unreleased says "the preset is `arkiv-entity-v3/glm-v2@2`" and, 94 lines up, "the preset is `arkiv-entity-v3/glm-v2@4`", and at :26 "`@5`" | `genesis_authoring.rs:35-39`: only @5 and @4 exist; `@2` is refused (genesis-bundle-v1.md:27-28) | Unreleased should state the net result once: delete the preset clauses at :208 and :302 |

#### [D1] 2. Redundant docs (groups; source of truth first)

#### G1. Plans next to their implementation docs

| Plan file (lines) | Status line in the file | Source of truth | Verified stale content | Action |
|---|---|---|---|---|
| docs/signed-proposer-v1-plan.md (528) | :3 "Status: approved and implemented"; :9-13 "The node has since been unified: there is no `arkiv-v2` cargo feature ... (the 'legacy' codecs and the feature gate it mentions no longer exist)" | docs/signed-proposer-v1.md (322) | :25-26 "They are not Ethereum transactions, Ethereum JSON-RPC" vs signed-proposer-v1.md:8-10 and `signed/ethereum.rs`, `signed/rpc.rs`; :307 "Complete canonical signed transaction 256 KiB" vs `signed/protocol.rs:27` `TX_CAP = 384 * 1024`; machine context at :102-106 ("The neighboring `/arkiv` manifest", "Local rustc 1.96", "the local cache contains signer-local 2.4.1"); hand-off instructions at :458-459, :475-477 ("No live deployment/reset is part of this implementation handoff", "Commit/push only the authorized repositories") | DELETE. First copy four decisions (10 lines) into signed-proposer-v1.md as "## Design decisions": k256 via alloy-primitives instead of alloy-signer (MSRV 1.90), no Reth pool (needs Ethereum tx traits), bodies in a separate `history.redb`, transaction nonce under its own host key `arkiv/txnon/` distinct from the create nonce. Fix signed-proposer-v1.md:14 (its only inbound link). -528 lines |
| docs/arkiv-system-compatibility-plan.md (144, 21 KB of very long lines) | :3 "implementation authorized by the user on 2026-09-24; delivered"; :5 "Superseded in part by user-defined columns" | docs/arkiv-v2-compatibility.md (contract) + docs/block-format.md (type table) | :23 "Source paths are relative to `/home/ubuntu/arkiv-network`"; :44 "`record.rs` currently has names up to 20 bytes, indexed values up to 64 bytes, and local tags bool=1/i64=2/u64=3/str=4/bytes=5" (now 32 / 128, golem-db type ids); :122-129 "1-2 engineering days ... Estimate: roughly 14-25 engineering days"; :142 "Decisions that need user input" | DELETE (or archive). One inbound code reference to update: `crates/arkiv-db/src/record.rs:866` cites it for the 128-byte profile - point that comment at block-format.md instead. -144 lines |
| docs/base-fee-v1-plan.md (78) | :3 "Selected by the user on 2026-09-25"; not implemented (`rg -i base_fee crates` finds only the constant `baseFeePerGas` in `signed/rpc.rs:1263`) | none (future work) | :47-52 cites local checkouts `arkiv/crates/arkiv-reth-chainspec/...`, `arkiv-op-geth/...` | MOVE to docs/proposals/base-fee.md (next to expiry-spill.md); reword :3 and :40 ("The user explicitly selected ...") to "Decision (2026-09-25): ..."; fix the two inbound links (signed-rpc-deployment.md:118, genesis-bundle-design.md:67) |
| docs/node-storage-plan.md (574) | :5-56 one 50-line paragraph of "Step X ... is done"; step table :374-380 with "(done)" on N0, N2, N3; :382-524 "N0a is done ... N0b-1 is done ... N0b-2 is done ... N0b-3 is done ... N2 is done ... N3a is done" (140 lines restating README/architecture/testing) | architecture.md (what exists), redb-experiment.md (N2 evidence), engine-storage-v1*.md (N3c), simulator-node.md / signed-proposer-v1.md (N4) | :16 broken anchor; :407-408 "its canonical fixed-width encoding is 33 bytes" (unpadded now); :499-500 "21-byte padded raw keys, 33-byte `RecordKey`, the 164-byte term composite" (state layout 2 removed all three, CHANGELOG.md:313-322); :49-51 "durable signing integration and broader N4 policy remain later work" vs README.md:219-220 "Durable signing and external Ethereum transaction admission are implemented" | It is the ONLY place the labels N0a..N6 are defined (:374-380) and 51 source comments use them (section 4). Reduce to docs/archive/node-storage-plan.md, or cut to an 80-line "Storage design decisions" (single writer, redb behind a narrow adapter, three commit outcomes, what a durable block transaction contains) and delete the status prose. about -490 lines |
| docs/snapshot-query-plan.md (1176) | :24 "Current implementation (milestones 0, 1 and 2, in memory)"; :73-78 "Not implemented: point and query proofs, the root in block headers, durable commits ... no SDK adapter exists" | for the charge table: this file (linked 6x as `#operation-cost-is-part-of-the-first-milestone` from testing.md:163,182,252, architecture.md:762, node-storage-plan.md:403, and cited by `crates/arkiv-db/src/query.rs:20`) | :73-78 contradicted by README.md:157-169 (eq/range proofs), README.md:155-156 (root in every header), README.md:170-177 (durable), README.md:186-188 (SDK adapter), and by its own :88-98; :155-160 "`RecordKey::ENCODED_LEN` ... 33 bytes ... `len ‖ bytes ‖ zero padding`" (`rg ENCODED_LEN crates/arkiv-db/src/record.rs` = 0 hits; record.rs:22 says unpadded); :184-185 `Value::MAX_INDEXED_LEN` (gone, see A3); :419 "Raw records (key padded to 20 + length) 21 / 44"; :66, :1019 "state-format-1" (now 2) | SPLIT: (a) `docs/metering.md` = lines 328-776 (classes, fixture weights, charge-point table, counting vectors; verified current: 31 classes = `meter.rs` `OperationClass`, `MODEL_VERSION = 3`), plus the planner summary that query.rs:20 points at; (b) `docs/query-model.md` = lines 148-326 trimmed to semantics + validation order (fix the key-encoding paragraphs); (c) archive the rest (1-147 status, 777-906 API listing that restates rustdoc, 907-1176 milestones and decision gates). about -520 lines live |

#### G2. Engine storage: spec vs report vs usage

| File | What it is | Action |
|---|---|---|
| docs/engine-storage-run.md (156) | Usage/lifecycle guide; defaults verified against `crates/arkiv-db/src/durable/mod.rs:53-57` (8 MiB / 16,384 entries / 32 MiB / 100,000 / 1,000,000), `cache.rs:47` (16 shards), `format.rs:38` (1 MiB) | KEEP as source of truth; rename to `durable-engine.md`. FIX :11-13 "The full/light demo still runs in memory; durable node restart is N4 ... there is no pruning" -> the simulator and proposer are durable hosts on top of this (README.md:215-221) and a serving floor exists (pruning-design.md) |
| docs/engine-storage-v1.md (592) | Pre-implementation spec: :41 "Code findings that constrain the implementation", :491 "Implementation sequence and validation gates", :586 "The implementer may choose a simple cache eviction structure ..."; :8 "N4 node integration is open"; :36 "Preserve state-format-1 bytes" | Keep only "Durable identity and bytes" (:137-232, the on-disk format: genesis/manifest/index layouts) and "Publication, failures and recovery" (:369-435) as `docs/durable-format.md` [CONSENSUS-adjacent: on-disk format text, move verbatim]; archive the remaining about 430 lines |
| docs/engine-storage-v1-report.md (232) | Generated measurement output (tables are the example's stdout) + process notes: :10 path `/home/ubuntu/arkiv-network/arkiv-state-db-experiment/target/perf-bench/esx`; :213-216 "Final integration validation, run by the integration lead after the cache-disabled edge-case fix, passed with **807 tests** ... checked against the local `target/engine-storage-workspace-tests-final.log`"; :208 "It did not rerun unchanged crash tests" | MOVE to `docs/benchmarks/2026-10-09-engine-storage.md`; delete :187-232 ("Validation commands") |

#### G3. Genesis bundle: design vs v1

- Source of truth: docs/genesis-bundle-v1.md (257; verified current: preset `arkiv-entity-v3/glm-v2@5` = `genesis_authoring.rs:35`, format/protocol 5, 128 expirations = `signed/protocol.rs:44`).
- docs/genesis-bundle-design.md (618): :3-9 says of itself "The 'baseline' and 'stages' sections below are the design record of 2026-09-25; the current wire format is in genesis-bundle-v1.md". Stale: :33 (and the example at :137) "the immutable `arkiv-entity-v2/glm-v2@1` preset" (code: `@5`, `@4` only; v1 doc :27 "Versions 1 to 3 ... are refused"); :27-29 "external sorting, resumable bulk checkpoints ... remain future work" (bulk-prefill.md implements bounded external sorting); :554 CLI "`genesis check/lock/compile/inspect`" (actual: `lock | build | verify | stream-build | stream-verify`, `arkiv-proposer.rs:592`); :89 and :610 "16 deletions per block" (128 now).
- Action: MERGE design :15-44 ("Implemented envelope", 30 lines, with the preset fixed) into genesis-bundle-v1.md, because v1.md:3-4 delegates "the authoring commands" to that section; ARCHIVE the other about 585 lines; fix inbound links (README none; genesis-bundle-v1.md:4; examples README).


#### G4. Full/light demo: contract vs run instructions vs proof charges

| File | What it is | Verified stale content | Action |
|---|---|---|---|
| docs/full-light-demo.md (476) | Design contract written before the code (":3 Status: implemented bounded demo profile, 2026-09-24"); defines the labels D0-D3 (:455-458) and holds the only prose spec of the `eq-page-v1` proof (:226-343) | :6-7 "today every chain uses the single block format (version 1, protocol 1)" vs `block.rs:47,58` = 5; :355 "Request / response body 1 MiB / 8 MiB" vs `crates/arkiv-node/src/demo/wire.rs:6` `REQUEST_CAP = 4 * 1024 * 1024` and `scripts/full-light-demo.py:26`; :41-57 "Existing code and required additions" (a to-do table, all done); :447-476 "Implementation order and review gates" ("Later: durable node - Resume N3c/N4", done) | SPLIT: `docs/eq-proofs.md` = :226-343 + :344-404 (bounds/errors) + all of eq-proof-charges.md (124) [CONSENSUS-adjacent: proof byte layout, move text verbatim]; `docs/full-light-demo.md` = roles/trust/wire (:58-225) + the run guide; DELETE :41-57 and :412-476 (about 80 lines) |
| docs/full-light-demo-run.md (155) | A usage guide (not a run log, despite the name); commands verified against `scripts/full-light-demo.py` / `test-full-light-demo.py` options and `arkiv-demo.rs:45-53` | :138 "Actual chunked request bodies: exactly 1 MiB accepted and one byte over rejected" vs `scripts/test-full-light-demo.py:336-337` (uses `demo.REQUEST_CAP` = 4 MiB) | MERGE into full-light-demo.md as "## Run"; FIX :138 |
| docs/eq-proof-charges.md (124) | Normative counting rules + literal vectors (cited by `crates/arkiv-db/src/proof.rs`) | none found | MERGE into `docs/eq-proofs.md` (above) or KEEP |

#### G5. Dated reviews (docs/review-*-2026-09-27.md, 605 lines) + docs/node-design-review.html (38.9 KB)

All four begin with a banner that says they are out of date ("At that time the node had an `arkiv-v2` cargo feature ... The node now has one build").
Provenance tells: review-summary:8 "A coordinated review by three agents and the main reviewer"; review-node:172-173 "No live network verification or
deployment was performed by this review agent. The root review coordinates broader checks and release."

Spot-check of findings against today's code:

| Finding | Status today | Evidence |
|---|---|---|
| N1 (medium): full follower forgot an advertised header after a failed body fetch | fixed, regression test present | `crates/arkiv-node/src/signed/host.rs:865` `observe_certificate`; `crates/arkiv-node/tests/signed_http.rs:1083` `advertised_header_survives_failed_body_fetch_and_detects_later_conflict` |
| S2 (low): "encoded-object cache lookup is linear ... searches a `VecDeque` with `iter().position` while holding one mutex" (review-storage:65-71; review-summary:86-88) | resolved: sharded CLOCK cache of decoded nodes with a key index | `crates/arkiv-db/src/durable/cache.rs:14-23` (shards, per-shard mutex only for lookup/insert), `:47` `MAX_SHARDS = 16`; `rg 'iter\(\)\.position' cache.rs` = 0 |
| "Resource admission: JSON-RPC HTTP has no equivalent fixed concurrent-request gate. Requests can queue behind the node mutex" (review-summary:77-79) | largely resolved | `--read-concurrency`/`--read-deadline-secs` (`arkiv-proposer.rs:654-655`), `signed/reads.rs:90`; CHANGELOG.md:147-174 |
| "Initial range policy: at most 64 distinct matching terms and 64 rows" (review-proofs:121) | superseded: 1,000 / 1,000 | `proof/range.rs:204-205` (this is where README.md:167's stale "64" comes from) |
| "Numeric types: Engine i64/u64 ..." (review-proofs:118) | superseded: every indexable type | CHANGELOG.md:362-363 |
| "The active gas profile requires finite entity lifetimes" (review-node:97-98) | superseded: live permanent entities | CHANGELOG.md:185-189, `genesis_bundle.rs:28` `PERMANENT_EXPIRY` |
| S1 retention / "No pruning" (review-storage:57-63, review-summary:74-76) | partly: serving floor (`--state-retention`) exists, physical deletion still not | pruning-design.md; `arkiv-proposer.rs:653` |
| Validation commands `cargo test ... --features arkiv-v2` (review-proofs:156,160; review-node:148,167; review-summary:115,120) | cannot run: the feature no longer exists | `crates/arkiv-node/Cargo.toml` has no `[features]` |
| Source links pinned to line numbers (`.../durable/cache.rs#L124`, `.../durable/mod.rs#L618`, `versioned.rs#L14`) | lines have moved (cache.rs:124 is no longer the lookup) | - |

Action: the still-true "remaining design limits" (single proposer; completeness is relative to an authenticated index; no physical pruning; lazy
corruption discovery; cache limits are not RSS limits) are worth 15 lines in a `## Known limits` section of README or architecture.md. Then
MOVE the four files to `docs/archive/reviews/` (or DELETE; git history keeps them). `docs/node-design-review.html`: a one-file slide deck
("A database that can prove its answers.", 15 `<h1>/<h2>` slides, minified CSS/JS inline, 7 `rogue-one.eu` links) referenced only by
review-summary-2026-09-27.md:14 and :147, which also says "A matching public copy is maintained in the frontend at `public/node-design.html`":
DELETE from this repo (the published copy lives in the frontend repo). Total -605 md lines, -38.9 KB html. safe.

#### G6. Run logs and deployment notes for one machine

| File (lines) | What it is | Inbound links | Action |
|---|---|---|---|
| docs/rogue-one-deployment.md (86) | Log of the 2026-09-26 chain-9009 reset on host `rogue-one.eu`: endpoints, faucet account, `/home/ubuntu/.config/rogue-one-9009` (:41), `sudo /home/ubuntu/.config/rogue-one-current/compose` (:59), "Validation passed 962 workspace tests" (:81) | 0 from markdown (only node-design-review.html) | MOVE to `docs/runs/2026-09-26-rogue-one-chain-9009.md` or to the ops repo; not product documentation |
| docs/signed-rpc-deployment.md (120) | Log of a superseded deployment (chain 9005, `rpc.experimental.arkiv-global.net`); :20 "The user requested discarding the previous test chain"; :75 `/home/ubuntu/.config/arkiv-simulator/signed-rpc`; :78-79 documents a flag that "no longer exists"; :17 "Finite entity lifetimes are required" (false now); :4 "not Ethereum JSON-RPC" (JSON-RPC now served at `/`, README.md:411) | 0 | DELETE (or `docs/runs/2026-09-25-signed-rpc-chain-9005.md`) |
| docs/glm-accounts-live-verification.json (214) | Evidence blob of that same chain-9005 run | 1 (signed-rpc-deployment.md:112) | DELETE/MOVE together with it |
| docs/rogue-one-metrics.md (125) | Reference for the Prometheus exporter - this is real product documentation, but named after the host because the metric prefix is `rogue_one_` in code (`crates/arkiv-node/src/signed/metrics.rs`, 9 hits; `arkiv-proposer.rs` help text "Metrics use rogue_one_ prefix") | 0 from markdown (only node-design-review.html) | KEEP, RENAME to `docs/metrics.md` and link it from signed-proposer-v1.md. Renaming the prefix itself (`rogue_one_` -> `arkiv_`) is the real fix but breaks dashboards: behaviour-affecting, owner decision |
| docs/streaming-prefill-100k-validation.md (333) | One validation run (2026-10-19) of "the historical incremental sample import" (README.md:425-427 calls it historical); 6 lines with absolute `/home/ubuntu/arkiv-network/...` paths (:24, :217, :218, :220, :244, :248) | 3 | MOVE to `docs/runs/2026-10-19-streaming-prefill-100k.md`; drop the README sentence (README.md:425-427) |
| docs/bulk-prefill-scaling.md (193) | One measurement report (100k vs 200k); :181-182 "Raw evidence is retained in the parent workspace under `operations/runs/bulk-prefill-200k`" | 2 | MOVE to `docs/benchmarks/2026-10-xx-bulk-prefill-scaling.md` |
| docs/full-prefill-docker.md (221) | Log of one job (:3 "Started October 19, 2026 as detached container `arkiv-experiment-full-7m-20261019`"; :6-7 "At launch the job was downloading; no successful full conversion ... is claimed here") on "this host" (:29-32 `/home/ubuntu/arkiv-network/operations/runs/full-prefill-20261019`, :146-147, :198-199) | 0 | MOVE to `docs/runs/2026-10-19-full-prefill.md`; keep the generic part ("Build-once runner" and "Runner validation", :97-131) as a section of streaming-genesis-v2.md |
| docs/streaming-genesis-v2.md :215-253 ("Build-once measurements, 2026-10-20") and :298-380 ("Historical incremental validation, 2026-10-19": "The measurements below describe the previous incremental importer"; "Logs ... are retained under `operations/runs/producer-optimization` in the parent workspace"; "The complete Rust workspace test run passed 1,174 tests") | run logs embedded in a how-to | - | MOVE both sections (about 120 lines) to `docs/benchmarks/2026-10-20-streaming-genesis.md`; the how-to keeps :1-214 and :255-296 |
| docs/bulk-prefill.md :69-146 ("Validation and measurements") | run log with `/home/ubuntu/...` command lines (:138-142) | - | same: MOVE to the benchmark file; keep :1-68 (contract, row format, resource requirements) |
| docs/engine-storage-v1-report.md (232) | see G2 | 9 files | MOVE to docs/benchmarks/ (update the 9 links) |
| docs/redb-experiment.md (590) | Gate-by-gate experiment report for redb 4.3.0; cited from `Cargo.toml:30`, `crates/arkiv-db-redb/src/lib.rs:6` | many | KEEP (it is the justification of the pinned dependency) but it is a report: fix :1 title "(node step N2)", :205 "ext4 on this machine" |
| docs/golem-db-conformance-report.md (242) | Report of a conformance run at pinned revisions | 2 | KEEP, or move under docs/benchmarks/; its "Benchmarks" section (:100-200) restates numbers of `docs/benchmarks/2026-10-10-*.md` (e.g. index-layout/1-no-index 6,839 vs 17,444 in both) - keep a three-line summary and the links |
| docs/benchmarks/*.md (5 files, 879 lines) | dated, generated; correct home | - | KEEP as is |


#### G7. SDK / entity compatibility (three overlapping files)

| File | State | Action |
|---|---|---|
| docs/node-api-reference.md (283) + `crates/arkiv-node/src/signed/state-openapi.json` | Current; the source of truth for the JSON-RPC and REST surface, UDC entity model, error codes | KEEP |
| docs/sdk-1.0-compatibility-todo.md (138) | A real, current open-items list (ordering, offsets, parser duplication, expiry); hand-written tone | KEEP; rename `sdk-open-items.md`. :84-86 "The helper scripts ... live outside the repository on the development machine" - either move them into `scripts/` or drop the sentence |
| docs/sdk-compatibility.md (376) | :3-16 banner "**Pre-UDC document.** This page and `scripts/sdk-compat-smoke.{py,ts}` describe and test the payload-era SDK (0.8.x)"; :247-309 "Rogue One balance-independent estimation rollout (2026-09-26)" (a deployment log for chain 9008, superseded by chain 9009 per rogue-one-deployment.md:3); :287-289 "its explicit permanent-entity test is excluded because this chain requires finite rent" (permanent entities are live, CHANGELOG.md:185); :305-306 "947 tests ... 907 in the generic build" | REWRITE as `sdk-compatibility.md` for SDK 1.0: keep "Authenticated writes and execution" (:25-86), "Reads and subscriptions"/paging (:87-167), "Boundaries" (:168-230), "Full released indexer acceptance" (:311-376); DELETE the banner and :231-309. If `scripts/sdk-compat-smoke.{py,ts}` can no longer pass against the node (payload-era SDK), delete them too - flag for the scripts reviewer. about -95 lines |
| docs/arkiv-v2-compatibility.md (129) | Title says "Arkiv entity compatibility profile"; the file name and `scripts/arkiv-v2-demo.py` keep the retired name `arkiv-v2`. "Version inventory" table (:49-59) is wrong on four of seven rows (below) | FIX the table or delete it (block-format.md owns versions); rename to `entity-profile.md` |

Stale rows in docs/arkiv-v2-compatibility.md:

| Line | Doc says | Code says |
|---|---|---|
| :5 | "protocol version 2" | `block.rs:58` `PROTOCOL_VERSION = 5` |
| :53 | "Engine layout, canonical codec, state format: 1" | `lib.rs:184` `LAYOUT_VERSION = 2`, `state.rs:32` `STATE_FORMAT = LAYOUT_VERSION`; `codec.rs:60` `CODEC_VERSION = 1` |
| :54 | "Name / indexed-value / term-key bytes (hard caps): 32 / 128 / 164" | `record.rs:131` 32; `record.rs:584` ceiling 512 (128 is the default policy, `record.rs:868`); term key at most 548 (`record.rs:580-581`, `layout.rs:292`) |
| :56 | "Node block format / protocol: 1 / 1" | 5 / 5 (`block.rs:47,58`) |
| :59 | "Record-key encoding: 33 bytes" | 2 to 33 bytes, unpadded (`record.rs:22-23`) |

#### G8. Prefill / Sourcify / streaming family (9 files, 2,284 lines)

| File (lines) | State | Action |
|---|---|---|
| docs/genesis-bundle-v1.md (257) | current wire contract | KEEP (source of truth for bundles) |
| docs/streaming-genesis-v2.md (380) | how-to + two embedded run logs (G6) | KEEP :1-214, :255-296; move the logs |
| docs/bulk-prefill.md (146) | construction contract + run log (G6) | KEEP :1-68; could be folded into streaming-genesis-v2.md as "## How the state is built" |
| docs/genesis-adoption.md (176) | current (`--genesis-install adopt` = `arkiv-proposer.rs:626,855`); "Measurements" (:137-166) is one run of "the October 19 full run" | KEEP; fold as a section of streaming-genesis-v2.md, or keep; move :137-166 to benchmarks |
| docs/sourcify-v4-prefill.md (173) | current converter (row codec 3 = `genesis_bundle.rs:21`) | KEEP |
| docs/sourcify-prefill-converter.md (405) | :3-12 banner "**Outdated by user-defined columns.** This converter still emits the payload-era row codec 2 ... which current nodes refuse" (confirmed: `genesis_streaming.rs:121` and `genesis_bundle.rs:251` reject any codec but `ROW_CODEC = 3`). README.md:419-420 still advertises it as live ("A [Sourcify converter](docs/sourcify-prefill-converter.md) preserves dataset references"). Three other docs link into its "expiry policy" sections (:223-320), which are still the only description of far-future/unlimited prefill genesis | MOVE :191-320 ("Dependencies, expiry and funding", the two expiry policies) into sourcify-v4-prefill.md or genesis-bundle-v1.md; DELETE the rest (about 275 lines) together with `scripts/sourcify-prefill.py` + `scripts/test-sourcify-prefill.py` if the owner agrees the codec-2 converter is dead (scripts are another chunk); fix README.md:419-420 |
| docs/full-prefill-docker.md (221) | run book for one host; :156, :170, :209 tell the reader to run `--converter /snapshot/sourcify-prefill.py`, i.e. the codec-2 converter the node refuses | MOVE to docs/runs/ (G6); if kept, FIX the converter name |
| docs/bulk-prefill-scaling.md (193), docs/streaming-prefill-100k-validation.md (333) | reports (G6) | MOVE to docs/benchmarks/ or docs/runs/ |

#### G9. Simulator (4 files) and small notes

| File | State | Action |
|---|---|---|
| docs/simulator-node.md (141) | current; flags verified against `arkiv-simulator.rs` (`--unsigned-simulator`, `--resume`, `--start`, `--interval-ms`, `--admin-listen`, `--state-retention`, roles `inspect`, `inspect-light`), env `SIMULATOR_*` (3 of 3 exist). One number: :105-106 "at most 32 cells per row" vs `demo/workload.rs:23` `max_cells: MIN_ENTITY_CELLS` = 39 (`arkiv/schema.rs:81`) - the feed-size arithmetic at :106-107 needs redoing | KEEP; FIX :105-107 |
| docs/simulator-storage-profile.md (64) | record layouts of the simulator host profile (on-disk format) | KEEP; MERGE into simulator-node.md as "## Storage profile" (it is linked from there) |
| docs/simulator-proof-inspection.md (192) | inspection JSON contract; cited by `crates/arkiv-node/examples/verify_inspection.rs:3` | KEEP |
| docs/query-language-classification.md (68) | 0 inbound links; overlaps `crates/arkiv-node/src/simulator/query_language/README.md` | MERGE into simulator-node.md ("## Text queries") |
| docs/state-tree-explorer-api.md (85) | :3-24 repeats node-api-reference.md's "/docs and /openapi.json" intro; :24 "The signed API uses the Arkiv v2 layout" (retired name) | MERGE the endpoint section (:26-85) into node-api-reference.md; delete :1-25 |
| docs/bricks-sample.md (42) | 0 inbound links; :6 "finite lifetimes" (no longer required); overlaps signed-proposer-v1.md:296-322 ("Named sample namespaces") | MERGE both into one `docs/examples/sample-namespace.md`; -40 lines from signed-proposer-v1.md |
| docs/experimental-prekucki-v1.md (215) | Describes the *current* key layout (state layout 2) but is titled "Experimental: ... (prekucki-v1)" and says :3-5 "Branch `experimental-prekucki-v1` ... `main` is unchanged". It is merged: `lib.rs:184` `LAYOUT_VERSION = 2`, CHANGELOG.md:313 "Unpadded, prefix-free keys (#2)" in 0.1.0-alpha.2. 5 inbound links | RENAME `docs/key-encoding.md`, title "Trie keys: variable-length, prefix-free (state layout 2)"; delete :3-5; keep "Why not the issue's design" as the decisions record. The codename "prekucki-v1" appears in 7 files (0 in crates) |

#### [D1] 3. Plans / todos that are done

| File | Verified done (rg) | Left open | Action |
|---|---|---|---|
| docs/signed-proposer-v1-plan.md | steps 1-6 (:463-477): `crates/arkiv-node/src/signed/{protocol,pool,history,host,runtime}.rs` exist; tests `signed_protocol.rs`, `signed_pool.rs`, `signed_history.rs`, `signed_host.rs`, `signed_http.rs`; `scripts/signed-proposer-smoke.py` | nothing | DELETE after lifting 4 decisions (G1) |
| docs/arkiv-system-compatibility-plan.md | phases 1-4 (:122-125): `crates/arkiv-node/src/arkiv/{mod,schema}.rs`, 25 `Value` variants, `tests/arkiv_profile.rs`, fixture `crates/arkiv-db/tests/fixtures/arkiv/sdk-host.json` | phase 5-6 concern other repos | DELETE (G1) |
| docs/engine-storage-v1.md "Implementation sequence and validation gates" (:491-576) | slices 1-5: `durable/{format,cache,prepare,mod}.rs`, `tests/durable.rs`, `durable_failures.rs`, `arkiv-db-redb/tests/engine_{history,crashes,process}.rs`, example `engine_storage_experiment.rs`; :552 "V1 acceptance note (2026-09-24): the hard functional gates above passed" | deferred allocation attribution (stated in the report) | ARCHIVE the section |
| docs/full-light-demo.md "Implementation order and review gates" (:447-476) | D0-D3: `crates/arkiv-node/src/demo/*`, `crates/arkiv-db/src/proof.rs`, `scripts/test-full-light-demo.py`; "Later: durable node" also done | "Later: broader proofs and archives" | DELETE the section (30 lines) |
| docs/node-storage-plan.md step table (:374-380) | N0, N2, N3 marked done in the table itself; N1 = `demo/`; N4 = `simulator/` + `signed/` (README.md:215-221) | N5 (broader query proofs), N6 (physical pruning: pruning-design.md phase 2) | ARCHIVE; carry "N5/N6" forward as two bullets in a `## Not implemented` list |
| docs/snapshot-query-plan.md milestones (:907-1176) | 0-2 "met in memory" (their own headings); 3: `roots.rs`, `proof.rs`; 5: `durable/` | 4 partially (only Eq and Range proof profiles) | ARCHIVE the section |
| docs/genesis-bundle-design.md stages 1-5 (:551-598) | stage 1-4: `genesis_authoring.rs`, `genesis_bundle.rs`, `tests/genesis_prefill.rs`; stage 5 (chunks, scale): `genesis_streaming.rs`, `genesis_bulk.rs`, `GET /signed/v1/prefill/chunk` | object packs | ARCHIVE (G3) |
| docs/pruning-design.md "Phases" (:269-279) | phase 0: `crates/arkiv-db-redb/examples/reachability_report.rs`, `durable/reachability.rs`; phase 1: `durable/retention.rs`, `--state-retention` | phase 2-3, correctly marked "designed, not implemented" | KEEP (accurate) |
| docs/proposals/expiry-spill.md | :3 "Status: implemented in this repository on 2026-11-17": `exec.rs:321` `EXPIRY_SPILL_WINDOW = 64`, `signed/protocol.rs:44` `MAX_EXPIRY_PER_BLOCK = 128` | it is addressed to the upstream team | KEEP (short, dated, says what it is) |
| docs/testing.md "Planned ..." sections (:491-560) | see T11 | snapshot import, wall-clock isolation | DELETE |
| docs/sdk-1.0-compatibility-todo.md | n/a - every item is still open | - | KEEP |
| docs/base-fee-v1-plan.md | not done (no implementation) | all | MOVE to docs/proposals/ |

#### [D1] 4. Style tells

1. **Undefined step labels.** `rg -n 'milestone|N0[a-z]-|step N|Phase [0-9]' crates -g '*.rs' | wc -l` = 54; with the wider pattern
   `rg -n -i 'milestone[- ]?[0-9]|\bN[0-6][a-d](-[0-9])?\b|node step|\bstep N[0-6]' crates -g '*.rs' | wc -l` = 52 (51 real: `typed.rs:855` "node step" means a trie step).
   Top labels in source: `N0a` 11 (all in `crates/arkiv-db/tests/trace_regression.rs`), `N3b` 8, `milestone-2` 8, `N3c` 6, `milestone-1` 5, `N0b-2`/`N0b-3`/`N3d`/`N4`/`N1` 2 each.
   The 86 "phase 0/1/2" hits are legitimate test phases in `isolation.rs`/`query_isolation.rs`, not plan labels.
   Four of the source uses are also stale: `crates/arkiv-db/src/engine.rs:69, 484, 860` and `state.rs:182` "Test aid, until node step N3c ..." (N3c is done).
   In docs: `node step` 94 occurrences in 10 files; `N3b` 73/10; `N4` 42/11; `N3c` 38/11; `N2` 32/10; `N0b-1` 24/6; `N3d` 24/9; `N0b-2` 21/6; `N0b-3` 21/6; `milestone N` 27/7; `D0-D3` 16/6 (about 365 label uses in all).
   Definitions exist only in: `docs/node-storage-plan.md:374-380` (N0-N6) and `docs/snapshot-query-plan.md:916-1090` (milestones 0-5); `docs/full-light-demo.md:455-458` (D0-D3).
   Action: replace each label by what it denotes ("since the engine moved to tries", "the codec", "durable engine") - mechanical, safe. README alone has 28 (`rg -o -i 'milestones?[- ][0-9]|\bN[0-6][a-d]?(-[0-9])?\b|node[- ]step' README.md | wc -l`).
2. **"The user ..." / agent narration** (13 places): vendor/README.md:3; docs/base-fee-v1-plan.md:3, :40; docs/arkiv-system-compatibility-plan.md:3, :142; docs/signed-rpc-deployment.md:20; docs/full-light-demo.md:21 ("The user-requested distinction"); docs/glm-gas-v1.md:117 ("the user-chosen rule"); docs/review-summary-2026-09-27.md:8 ("three agents and the main reviewer"); docs/review-node-2026-09-27.md:172 ("this review agent"); docs/engine-storage-v1-report.md:213 ("run by the integration lead"); docs/engine-storage-v1.md:586 ("The implementer may choose"); docs/signed-proposer-v1-plan.md:459 ("this implementation handoff").
3. **"N tests passed" as evidence**, 12 places with 9 different workspace totals (807, 903, 947, 960, 962, 974, 1,161, 1,174, 1,201): CHANGELOG.md:340; docs/rogue-one-deployment.md:81; review-summary:118; signed-rpc-deployment.md:100; bulk-prefill.md:71; sdk-compatibility.md:269, :305; streaming-genesis-v2.md:378; streaming-prefill-100k-validation.md:321-322; engine-storage-v1-report.md:214; review-storage:109; review-node:145. Today: 1,072 `#[test]`/`#[tokio::test]` attributes (`rg -c '#\[(tokio::)?test' crates -g '*.rs'` summed). DELETE the counts; CI is the evidence.
4. **Machine-specific content**: `rogue-one`/`rogue_one` in 12 doc files (20 hits in rogue-one-metrics.md, 9 in rogue-one-deployment.md, 5 in node-design-review.html) and in code (`signed/metrics.rs` 9, `arkiv-proposer.rs` help 1); `/home/ubuntu/...` in 9 doc files, 27 lines (benchmarking.md:25-26,41,50,138; bulk-prefill.md:138-142; streaming-prefill-100k-validation.md:24,217-248; full-prefill-docker.md:32,146-147,198-199; engine-storage-v1-report.md:10; arkiv-system-compatibility-plan.md:23; rogue-one-deployment.md:41,59; signed-rpc-deployment.md:75; benchmarks/2026-10-11-prekucki-v1.md:27); "the parent workspace" `operations/runs/...` in streaming-genesis-v2.md:324,352-353, bulk-prefill.md:126, bulk-prefill-scaling.md:181-182; "on this machine"/"this host" in testing.md:343, redb-experiment.md:205, pruning-design.md:186, full-prefill-docker.md:29. `scripts/bench-compare.sh:24-25,260` hard-codes the same `/home/ubuntu` defaults (flag for the scripts reviewer). FIX: `$REPO`, `$WORK` placeholders.
5. **docs/node-design-review.html**: see G5. DELETE.
6. **JSON artifacts in docs/**: `docs/glm-gas-v1-calibration.json` (100 lines) is the stdout of `cargo run -p arkiv-node --example gas_calibration`; numbers equal the table in glm-gas-v1.md:328-334; referenced by nothing (`rg glm-gas-v1-calibration` = 0 hits outside itself) -> DELETE (the doc already says how to reproduce). `docs/glm-accounts-live-verification.json` (214): evidence of the superseded chain-9005 deployment, referenced only by signed-rpc-deployment.md:112 -> DELETE with it.
7. **README shape**: 229 lines before the first `##`; one paragraph is 21 lines of receipt arithmetic (README.md:88-114); three status paragraphs contradict one another's scope (R7); the last 26 lines (407-432: gas profile, SDK, genesis, API reference) sit under `## Development`. See R10 for the target shape.
8. **CHANGELOG**: `## Unreleased` is 302 lines (9 entries, two of them "**Breaking:** block format version N") against 45 and 40 lines for the two releases, while `Cargo.toml:16` still says `0.1.0-alpha.2`. Entries that narrate process rather than change: :339-345 "### Validation - 1,161 tests pass ... An independent adversarial review of the proof paths found nothing above low severity"; :149-153 ("`scripts/signed-proposer-smoke.py` failed about one run in four") and :164-168 (stress-run numbers) are acceptable as motivation but long. Otherwise the changelog is the best-written file in the chunk: concrete, consensus impact called out. Action: cut a release (or split Unreleased by version), delete :339-345, fix O13/O14.
9. **Docs that restate code**: snapshot-query-plan.md:777-906 (signature listing of `Engine`/`Branch`/`Snapshot`, i.e. rustdoc); architecture.md:492-532 (enumerates every `try_` method); testing.md tables whose cells paraphrase each test body (46 lines over 400 characters, the longest 2,128: testing.md:445). Prefer one sentence per file plus the file's own `//!` header.
10. **File names carrying retired versions**: `glm-gas-v1.md` documents profile `glm-v2` / rent rule `glm-rent-v3`; `glm-accounts-v1.md` is titled "Native GLM accounts (`glm-v2`)"; `arkiv-v2-compatibility.md` documents `arkiv-entity-v3`; `signed-proposer-v1.md`, `engine-storage-v1.md`, `genesis-bundle-v1.md`, `streaming-genesis-v2.md`. Drop the suffixes when moving files (`glm-gas.md`, `glm-accounts.md`, `entity-profile.md`, `signed-proposer.md`, ...). needs-care: 60+ inbound links and 23 source comments cite docs by path (`rg -o 'docs/[a-z0-9./-]+\.md' crates -g '*.rs'`: block-format.md 7, state-trie.md 6, snapshot-query-plan.md 2, redb-experiment.md 2, genesis-bundle-v1.md 2, node-storage-plan.md, eq-proof-charges.md, simulator-proof-inspection.md, arkiv-system-compatibility-plan.md 1 each).
11. **A consensus input lives under docs/**: `crates/arkiv-node/src/genesis_authoring.rs:49` `include_str!("../../../docs/examples/genesis-bundle/profiles/fixed-glm-v2.toml")` is the `@5` preset expansion (the `@4` one is in `src/genesis_presets/`). Editing or moving the "example" changes genesis hashes / breaks the build. Move the file to `src/genesis_presets/arkiv-entity-v3-glm-v2-5.toml` and let a test assert the docs copy is byte-identical. [CONSENSUS] needs-care: bytes must not change.

#### [D1] 5. README accuracy

- Commands: all ten Development commands resolve (see "Verified OK" under README above). There is no quickstart for `arkiv-proposer` or `arkiv-simulator` in the README; the first runnable thing mentioned is `python3 scripts/full-light-demo.py` at line 207, for the volatile demo.
- `--allocations-file allocations.json` (README.md:409) is JSON (`arkiv-proposer.rs:139-151`), correct.
- Crate table: `arkiv-node` row stale (R5); `reth-trie` row describes an unused crate (V2); `arkiv-db-memory` and `arkiv-db-redb` rows correct; `arkiv-db` row correct but omits `proof` (the `eq-page-v1` / `range-complete-v1` proof module, `crates/arkiv-db/src/proof.rs`, `proof/range.rs`) and `explore`.
- Relative links: every `[..](path)` target in README.md and docs/architecture.md exists (0 broken files across all 67 markdown files checked); 1 broken anchor in README (line 154) and 1 in architecture.md (line 138) - see "Broken anchors".
- README.md:419-420 advertises the codec-2 Sourcify converter as usable (G8).
- README.md:167 range caps (R3), :431 method count (R4), :63-64 key encoding (R1), :70 cell-name and indexed-value limits (R2).

#### [D1] Proposed target layout

```
README.md                      ~200 lines (from 432): intro, quickstart, status table, workspace, development, doc index
CHANGELOG.md                   keep; release cut, O13/O14 fixed
docs/
  architecture.md              ~620 (drop "Next increments", history prose; fix A1-A11)
  block-format.md              931 (fix O1, heading)                       [normative]
  state-trie.md                527                                         [normative]
  key-encoding.md              ~205 (was experimental-prekucki-v1.md)      [normative]
  metering.md                  ~450 (charge table out of snapshot-query-plan.md) [normative]
  query-model.md               ~180 (semantics + validation order, same source)
  eq-proofs.md                 ~300 (eq-page-v1 from full-light-demo.md + eq-proof-charges.md) [normative]
  range-proofs.md              180
  durable-engine.md            ~250 (engine-storage-run.md + format/recovery sections of engine-storage-v1.md)
  redb-experiment.md           590
  pruning-design.md            278
  project-namespaces.md        ~300, single-block-query-proofs.md ~320 (contracts, status sections removed)
  entity-profile.md            ~120 (was arkiv-v2-compatibility.md), entity-expiration.md 265
  glm-gas.md 382, glm-accounts.md 161
  signed-proposer.md           ~290 (+10 lines of decisions, -40 sample namespace)
  node-api-reference.md        ~340 (+ state tree endpoint), metrics.md 125 (was rogue-one-metrics.md)
  sdk-compatibility.md         ~280, sdk-open-items.md 138
  genesis-bundle.md            ~290 (v1 + "implemented envelope"), streaming-genesis.md ~260 (+bulk contract, adoption)
  sourcify-v4-prefill.md       ~300 (+ expiry policies)
  simulator-node.md            ~270 (+ storage profile, text queries), simulator-proof-inspection.md 192
  full-light-demo.md           ~330 (roles/trust + run guide)
  testing.md                   ~380 (oracle rules + per-file tables, no history, + 35 missing files)
  benchmarking.md              148 (paths parameterised), golem-db-conformance-report.md 242
  benchmarks/                  5 existing + engine-storage, bulk-prefill-scaling, streaming-genesis, genesis-adoption runs
  proposals/                   expiry-spill.md, base-fee.md
  examples/                    genesis-bundle/, far-future-prefill/, unlimited-prefill/, sample-namespace.md
  archive/ (or delete)         node-storage-plan.md, snapshot-query-plan.md (status + milestones), genesis-bundle-design.md,
                               engine-storage-v1.md (spec parts), reviews/ (4 files)
  runs/ (or ops repo)          rogue-one-deployment.md, streaming-prefill-100k-validation.md, full-prefill-docker.md,
                               deploy/README.md:34-67
DELETE                         signed-proposer-v1-plan.md (528), arkiv-system-compatibility-plan.md (144),
                               signed-rpc-deployment.md (120), glm-accounts-live-verification.json (214),
                               glm-gas-v1-calibration.json (100), node-design-review.html (38.9 KB),
                               sourcify-prefill-converter.md minus its expiry sections (~275),
                               state-tree-explorer-api.md:1-25, bricks-sample.md / query-language-classification.md (merged)
```

Approximate effect on the live docs tree (docs/*.md is 54 files, 15,899 lines today; 16,928 lines with benchmarks/ and proposals/):

| Disposition | Lines |
|---|---|
| Deleted outright (plans done, superseded deployment, dead converter text, JSON, merged stubs) | about 1,100 md + 314 json + 38.9 KB html |
| Archived (plans' status/milestones, design-before-code specs, dated reviews) | about 2,650 |
| Moved to runs/, benchmarks/ or proposals/ (run logs, measurement reports, embedded log sections) | about 1,400 |
| Removed from README (279-390 test list, status prose) | about 230 |
| Removed inside kept docs (testing.md history + planned sections 125, architecture.md 85, contracts' stage lists 105, signed-proposer 45, sdk-compatibility 95, full-light-demo 80, duplicate intros lost in merges) | about 1,000 |
| Live docs after the change | about 9,700 lines in 31 files (from 15,899 in 54) |
