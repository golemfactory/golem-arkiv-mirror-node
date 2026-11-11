# Code quality: open items

A full read of the workspace (2026-11-17) found the core careful and the
scaffolding around it repetitive. The first pass removed dead code, duplicate
and tautological tests, stale and decorative comments, finished plan documents
and obsolete scripts. What follows was left because it is a refactor, changes
behaviour, or needs a decision.

## Defects

| Where | Defect | Why it is not fixed yet |
| --- | --- | --- |
| `crates/arkiv-node/clippy.toml` | `Branch::checkpoint` and `Branch::seal` panic on a poisoned branch and are not on the ban list; `exec.rs`, `arkiv/mod.rs` and `genesis_bundle.rs` call them | a panic becomes an error path |
| `signed/rpc.rs` `uint` | accepts `"0x+1"` (`u64::from_str_radix` takes a sign) | changes what the RPC accepts |
| `signed/ethereum.rs` | a bad y-parity is reported as `GasPriceBelowMinimum` | changes an error code clients see |
| `signed/runtime.rs` + `signed/host.rs` `submit` | a submitted transaction is validated twice, the second time under the node lock | needs a validated-transaction type |
| `demo/runtime.rs` | `serde_json::to_vec(..).unwrap_or_default()` turns a serialisation failure into an empty 200 | changes a response |

## Refactors, by payoff

1. **Shared test support in `arkiv-db`.** `ns`, `receipt`, `traced`, `put`,
   `delete`, the owner constants, the record/query reference model (seven
   copies) and the history and isolation oracles are re-typed per test file.
   `Branch` and `Snapshot` have the same read methods and no common trait, so
   about ten test files write a delegating `Reader`. One integration-test
   binary per crate would let the helpers be shared without `allow(dead_code)`.
2. **Store test doubles.** `FaultyStore` now forwards `stage_objects` and
   `relabel`; the eight hand-written `impl VersionedStore` wrappers in
   `arkiv-db/tests`, `arkiv-db-redb/tests` and `arkiv-node/tests` can use it.
3. **Charge vectors pinned twice.** The remaining cost tests in
   `tests/records.rs` and `tests/queries.rs` overlap `query_metering.rs`;
   vectors that exist only there must be moved, literal by literal, before
   the tests go.
4. **`arkiv-node` errors.** `Error(&'static str)` with 107 codes and 305
   `map_err(|_| ..)`; the HTTP status is chosen by string match. An enum whose
   `as_str()` returns today's strings keeps the wire format.
5. **Three node generations.** `signed` imports from `demo`; `signed/runtime.rs`
   restates about 330 lines of `simulator/`; hex and quantity parsing exist in
   about twenty places with different strictness. Neutral `hex`, `wire`,
   `http` modules.
6. **`signed/host.rs`.** The height-zero setup is written three times and
   there are four many-argument constructors; a `Launch` struct.
7. **`bin/arkiv-proposer.rs`.** `run()` is about 660 lines; one flag whitelist
   serves every subcommand.
8. **`engine.rs`.** Thirty infallible methods are `in_memory(self.try_x(..))`
   and only tests call them; the read methods re-list the same arguments.
9. **Consensus logic written twice in `arkiv-node`.** Due-schedule
   maintenance and expiry resolution (`arkiv/mod.rs` and `exec.rs`, differing
   at `u64::MAX`), account debit and burn.
10. **Test support in release builds.** `arkiv_db_storage::testing` and
    `arkiv_db_redb::testing` are ordinary public modules; a `testing` feature.
11. **Scripts.** Eight smoke launchers repeat the same node harness.

## Decisions

- **Licence.** `crates/arkiv-node/src/simulator/query_language/` is adapted
  from GPL-3.0-or-later code; the workspace declares no licence.
- **`vendor/reth-trie`** is a workspace member nothing depends on.
- **The raw key-value facade** (`Database`, `KvStore`, `MemoryStore`) is used
  only by its own tests and one example.
- **`prepare_import`, `import_checkpoint`, `initialize_from_staged`** have no
  production caller.
- **The demo profile** (about 2,100 lines) if it is no longer needed.
- **`docs/genesis-bundle-design.md`** is the original design; the contract is
  `genesis-bundle-v1.md`. Archive or delete once nothing in it is unique.
- **`AGENTS.md`** at the repository root.
