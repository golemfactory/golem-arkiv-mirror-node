# Code quality: open items

What is left after the cleanup and refactor passes of 2026-11-17/18, which
acted on the [code-quality review of 2026-11-17](reviews/2026-11-17/README.md).

## Refactors

1. **`arkiv-node` tests.** 32 integration-test binaries; several temp paths
   are keyed by process id and the SDK cursor cache is process-global, so
   they cannot share one binary yet. The first `signed_http.rs` test runs
   four stages on one chain. `SlowStore` in `signed_reads.rs` can use
   `FaultyStore::set_object_read_hook`.
2. **`rpc_reads` parser.** The request-syntax plumbing in
   `signed/rpc_reads/syntax.rs` repeats per query form.
3. **Infallible engine methods.** The 30 `Snapshot`/`Branch` methods that
   wrap a `try_` form are called only by tests and examples (about 900
   call sites) and are part of the public API.
4. **`BoundaryStore`** in the redb `engine_process` test needs a pause after
   a real commit, which `FaultyStore` does not offer.
5. **Scripts.** Eight smoke launchers repeat the same node harness.

## Decisions

- **The demo profile** (about 2,100 lines) if it is no longer needed.
- **`docs/genesis-bundle-design.md`** is the original design; the contract is
  `genesis-bundle-v1.md`. Archive or delete once nothing in it is unique.
- **`AGENTS.md`** at the repository root.

## Coverage gaps

No test covers:

- adoption resuming after the durable relabel (staged store relabelled,
  `node.redb` still headless; the `"resumed":true` path);
- `check_adopted` returning `IdentityMismatch`;
- `genesis_adopt::publish` refusing an initialised node;
- rollback of the creation-nonce counter and the due-schedule total after a
  failed batch.
