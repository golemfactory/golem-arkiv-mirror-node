# Vendored trie sources

The trie sources are vendored deliberately: path dependencies at pinned
upstream revisions, so the implementation is local. Only the KV storage
contract and the memory backend are standard-library-only.

## Alloy trie

`alloy-trie/` contains the complete published `alloy-trie` **0.9.4** Rust source,
manifest, benchmarks, README, and upstream licenses. Its published VCS metadata pins
[`alloy-rs/trie` at `04208a0e64ac347a2e463dd4c5935b20aa9ed4b4`](https://github.com/alloy-rs/trie/tree/04208a0e64ac347a2e463dd4c5935b20aa9ed4b4).
Upstream Rust source is unmodified, including its node encoding, Keccak hashing,
tests, and proof modules. The manifest's package-local profiling section is
removed because Cargo profiles belong at the workspace root; the original
manifest is retained as `Cargo.toml.orig`. `arkiv-db-trie` uses alloy's `RlpNode`
and `EMPTY_ROOT_HASH`, and its tests use alloy's node encoders,
`HashBuilder`, `ProofRetainer` and `verify_proof` as independent oracles; it
does not use `verify_proof` as its verifier (see `docs/state-trie.md`).

Upstream licenses these files under **MIT OR Apache-2.0**; its license files
and copyright notices are retained in the directory. The rest of this
repository is GPL-3.0-or-later (see the root `LICENSE`).
Other dependencies are resolved through Cargo and pinned in the root lockfile.

## Updating

Update deliberately against a pinned upstream revision. Compare the copied
files and dependency versions, preserve the licenses, and run upstream unit
tests plus our database tests. Do not silently replace the core with a registry
dependency: the path dependency is intentional so the implementation is local.
Exclude `alloy-trie` from formatting and Clippy changes to preserve its source.
