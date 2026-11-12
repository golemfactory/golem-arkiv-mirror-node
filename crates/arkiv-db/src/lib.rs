//! The embedded database entry point.
//!
//! [`Engine`] is the application-facing block engine. Every read and write
//! names a namespace, is metered with checked integer arithmetic under
//! counting-rule version [`MODEL_VERSION`], and runs against an immutable
//! snapshot or a candidate branch. Each namespace holds a raw byte map and,
//! as sibling fields, a typed record store: key bindings, rows and a term
//! index that every record write maintains in the same operation. The
//! declared query subset (typed equality, inequality, ordered ranges,
//! string prefixes, existence and negation in an ordered flat DNF) is
//! evaluated lazily in record-id order with projection and keyset
//! pagination; `count` and `terms` (the distinct indexed values of an
//! attribute in value order, read positionally) are separate metered
//! reads; a [`Cursor`] is bound to one snapshot of one engine, or to
//! one branch state, and never re-resolves head. Instance parameters are
//! [`Limits`]. The engine keeps its state in structurally shared Merkle
//! Patricia maps (`arkiv_db_trie::MerkleMap`), and nothing reachable
//! through it bypasses namespace or metering rules; the metered data-plane
//! reads are `get`, `scan_prefix`, `descriptor`, `get_record`, `query`,
//! `count` and `terms`, and the only other calls that read state are count
//! and map-identity statistics that return no keys or values, a hidden
//! index-consistency test aid, and the host-state calls, which see only
//! host-owned entries.
//!
//! The host owns consensus, block numbers and expiry: [`expire_records`]
//! deletes expired records through the host path in the block where they
//! expire, sweeping the expired heights in ascending order (`terms` reads
//! the smallest live height, its rows are deleted as `Eq` pages, then the
//! next height), and the engine assigns `$`-cells no meaning. Each state
//! also holds a small host-owned map (32-byte keys, values of at most
//! [`HOST_VALUE_MAX`] bytes) for the host's own deterministic schedules,
//! reachable only through unmetered `try_host_*` calls and never through
//! the data plane.
//!
//! Every state has a logical state root ([`Snapshot::state_root`],
//! [`SealedCandidate::state_root`]): the digest of its [`StateHeader`],
//! which commits to the catalog, every namespace's maps and the host map
//! through their trie roots (see [`state`]). It depends only on the logical
//! content.
//!
//! Every [`Branch`] and [`Snapshot`] method that walks state has a `try_`
//! form returning `Result<_, StorageError>`. On an engine built in memory
//! they never fail; on one that reads committed map nodes they fail on
//! storage errors, and a storage error poisons a branch (see
//! [the engine's fallible surface](engine#fallible-surface)).
//!
//! A host may stop a read it abandons (a wall-clock deadline, a departed
//! client) with an [`Interrupt`]: a read whose budget opted in stops at its
//! next charge inside the interrupt's scope. Block execution never opts in
//! (see [`interrupt`]). A local query or count read may also opt in to
//! planned evaluation ([`Budget::planned`]: each AND-group is driven by its
//! cheapest index source, with the same results) and to read-side admission
//! bounds ([`Budget::with_query_limits`]); block execution never opts in to
//! either.
//!
//! [`codec`] is the canonical binary encoding (codec version
//! [`CODEC_VERSION`]) of the engine's types, with the domain-separated
//! [`digest`] that every block and state hash uses.
//!
//! [`proof`] supplies bounded complete equality pages, complete ordered
//! ranges without pagination, and a stateless verifier, with separately
//! versioned advisory proof-read accounting. [`durable::DurableEngine`]
//! persists the same state and historical proof material with lazy reads, a
//! bounded decoded-node cache and bounded preparation. The ordinary
//! [`Engine`] remains volatile.

pub mod codec;
pub mod durable;
pub mod engine;
pub mod host;
pub mod interrupt;
mod layout;
pub mod meter;
pub mod namespace;
mod nodes;
pub mod proof;
pub mod query;
pub mod record;
pub mod state;
mod typed;

pub use arkiv_db_storage::{
    Entry,
    versioned::{StorageError, StorageErrorKind},
};
pub use codec::{
    B256, CODEC_VERSION, Decode, DecodeError, Decoder, Encode, Encoder, decode_exact, digest,
    encode,
};
pub use engine::{
    Branch, Checkpoint, DEFAULT_NAMESPACE_CAP, Effect, Engine, EngineError, Executed,
    HOST_VALUE_MAX, HostKey, HostStateError, Metered, ReadResult, Rejected, SealedCandidate,
    Snapshot, SnapshotId, WriteOp, WriteResult,
};
pub use host::{BlockNumber, ExpiryHalt, expire_records, try_expire_records};
pub use interrupt::Interrupt;
pub use meter::{
    Budget, Cost, Ledger, LedgerEntry, MODEL_VERSION, OperationClass, Outcome, PhysicalTrace,
    PriceSchedule, QueryLimits, Receipt, ReceiptKind, Rejection,
};
pub use namespace::{NamespaceDescriptor, NamespaceId, NamespaceName, OwnerId, Revision};
pub use query::{CellSelect, Cursor, Literal, Projection, Query, QueryFault, QueryPage, Term};
pub use record::{
    ArgumentFault, Cell, CellKind, CellName, Float32, Float64, Limits, NameFault, RecordId,
    RecordKey, Row, TypeTag, Value,
};
pub use state::{MapKind, NamespaceRoots, STATE_FORMAT, StagedObject, StateHeader};

/// Version of the logical trie layout and of the state and proof formats
/// built on it. Version 2 stores term, record and raw keys unpadded
/// (prefix-free variable-length keys); a store, genesis or proof of
/// version 1 is rejected before interpretation.
pub const LAYOUT_VERSION: u32 = 2;
