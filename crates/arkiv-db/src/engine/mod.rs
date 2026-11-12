//! Block-oriented execution over immutable snapshots of authenticated state.
//!
//! The engine keeps every published state as an immutable [`Snapshot`]. A
//! host begins a [`Branch`] from the current head, executes namespace-scoped
//! write operations in order with per-operation budgets, takes checkpoints
//! for transaction rollback, seals the candidate and publishes it. Publication
//! swaps in one complete state, so every namespace change and descriptor of a
//! block becomes visible together, and rejects candidates whose base is no
//! longer the head.
//!
//! Snapshot identities are internal and sequential; they are not block
//! numbers. The host maps blocks to snapshots (see [`crate::host`]) and owns
//! consensus, finality, signature validation and admission policy. The engine
//! trusts the `actor` a host passes with each write and enforces namespace
//! ownership against the branch state at that operation's position.
//!
//! The only metered data-plane entry points are `execute`, `estimate` (and
//! their `_host` variants), `get`, `scan_prefix`, `descriptor`, `get_record`,
//! `query`, `count` and `terms` on [`Branch`] and [`Snapshot`]. No method
//! exposes a catalog, record map or index. The unmetered
//! statistics (`namespace_count`, `record_count`, `typed_record_count`,
//! `term_count`, `records_shared_with`, `typed_shared_with`) reveal only
//! counts and map-root identity for hosts and tests, and the hidden
//! `index_consistent` test aid rebuilds a namespace's index to check it;
//! none of them returns keys or values and none may be exposed as an RPC
//! read.
//!
//! # State and roots
//!
//! Every state is a set of Merkle Patricia maps (`arkiv_db_trie::MerkleMap`,
//! with the key and leaf encodings of [`STATE_FORMAT`]): the namespace
//! catalog, whose leaf for each namespace holds its descriptor, record
//! allocator and the roots of its raw record, key-binding, row and term
//! maps, and the host-state map. Updates copy one key path per map and
//! share everything else, so every published snapshot stays valid and
//! cheap. [`Snapshot::state_root`] and [`SealedCandidate::state_root`] are
//! the digest of the [`StateHeader`] (see [`crate::state`]); they depend on
//! the logical content only, and computing them hashes only the nodes built
//! since they were last hashed.
//!
//! # Host state
//!
//! Besides the namespaces, every state holds a host-owned map from 32-byte
//! keys to values of at most [`HOST_VALUE_MAX`] bytes, for work the host
//! schedules deterministically (such as expiry due counts). It is part of
//! the state like the catalog: checkpoints and rollbacks restore it, a
//! sealed candidate carries it, every published snapshot keeps its own
//! version, and its root enters the state root. It is reachable only
//! through the unmetered host-internal calls
//! `Branch::{try_host_get, try_host_put, try_host_remove, try_host_range}`
//! and `Snapshot::{try_host_get, try_host_range}`; no write operation, host
//! path, estimate, metered read or statistic reads or changes it, so it
//! never changes a receipt. A host must never serve it to users.
//!
//! # Fallible surface
//!
//! Every [`Branch`] and [`Snapshot`] method that walks state, metered or
//! not, has a `try_` form returning `Result<_, StorageError>` whose `Ok`
//! value is exactly what the infallible method returns. A [`StorageError`]
//! is an availability failure of the storage beneath the state: a map node
//! that is missing, corrupt or unreadable. It is never a rejection, a
//! receipt, a missing record or an empty result, and a host must not record
//! it in a block.
//!
//! An engine built by [`Engine::new`] keeps its state in memory, its maps
//! never hold a stored link, and the `try_` forms never fail; the
//! infallible methods delegate to them. An engine opened from a committed
//! state ([`Engine::from_state`], a test aid, or [`crate::durable`]) reads
//! map nodes lazily through an
//! `arkiv_db_storage::versioned::ObjectReader`, and there every `try_` form
//! can fail. On a snapshot a storage error just fails that call. On a
//! branch, a storage error inside an operation restores the state from
//! before the operation, records no receipt, adds nothing to the block's
//! spend, and **poisons** the branch: every later `try_` call returns the
//! poisoning error, and `try_checkpoint`, `rollback_to`, `set_pricing` and
//! `try_seal` return [`EngineError::Poisoned`]. The host discards a poisoned
//! branch. The node calls only the `try_` forms.
//!
//! This engine is volatile; [`crate::durable`] persists the same state.

mod branch;
pub mod explore;
mod ops;
mod read;
mod snapshot;
mod staging;
mod write;

pub use self::{
    branch::{Branch, Checkpoint},
    ops::{Effect, Executed, Metered, ReadResult, Rejected, WriteOp, WriteResult},
    snapshot::Snapshot,
};

use std::{
    collections::BTreeMap,
    fmt,
    sync::{
        Arc, OnceLock,
        atomic::{AtomicU64, Ordering},
    },
};

use arkiv_db_storage::versioned::{ObjectReader, StorageError, StorageErrorKind};
use arkiv_db_trie::Probe;

use crate::{
    codec::B256,
    layout::{Catalog, HostMap, NamespaceState},
    meter::{Cost, PriceSchedule, Receipt},
    namespace::NamespaceId,
    nodes::{Nodes, corrupt},
    record::Limits,
    state::{MapKind, STATE_FORMAT, StagedObject, StateHeader},
};

/// Default directory capacity: the largest number of namespaces an engine
/// admits. The directory is a Merkle Patricia map over 8-byte big-endian
/// ids, so whatever the capacity a lookup visits at most `2·8 + 1` = 17
/// nodes and a write (lookup plus update path) at most 34, and the constant
/// `DirectoryLookup` price reserves a bounded worst case.
pub const DEFAULT_NAMESPACE_CAP: u64 = 1 << 32;

pub(crate) static NEXT_INSTANCE: AtomicU64 = AtomicU64::new(1);
static NEXT_BRANCH_TOKEN: AtomicU64 = AtomicU64::new(1);

/// Longest value the host-state map accepts, in bytes.
pub const HOST_VALUE_MAX: usize = 256;

/// A key of the host-state map.
pub type HostKey = [u8; 32];

/// A refused host-state write. Nothing was changed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HostStateError {
    /// The value is longer than [`HOST_VALUE_MAX`] bytes.
    ValueTooLarge {
        /// The value's length.
        len: usize,
    },
}

impl fmt::Display for HostStateError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ValueTooLarge { len } => write!(
                formatter,
                "host value of {len} bytes exceeds {HOST_VALUE_MAX} bytes"
            ),
        }
    }
}

impl std::error::Error for HostStateError {}

/// A count held in a `u64`, as the `usize` the statistics return.
fn count(len: u64) -> usize {
    usize::try_from(len).expect("map sizes fit in usize")
}

/// Unwraps the result of a state walk of an engine whose state is entirely
/// in memory, where no storage beneath it can fail. The infallible methods
/// delegate to their `try_` forms through this; on an engine that reads
/// committed nodes (see [`Engine::from_state`]) they panic on a storage
/// error, so such a host calls only the `try_` forms.
pub(crate) fn in_memory<T>(result: Result<T, StorageError>) -> T {
    match result {
        Ok(value) => value,
        Err(error) => panic!("storage error on an infallible engine call: {error}"),
    }
}

/// Internal identity of a published state.
///
/// Sequential from [`SnapshotId::GENESIS`], never reused within an engine
/// instance, and distinct from any block number.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SnapshotId(u64);

impl SnapshotId {
    /// The empty state every engine starts from.
    pub const GENESIS: Self = Self(0);

    /// A stable sequence supplied by a durable manifest lookup. Existence is
    /// checked by the engine, not by this numeric constructor.
    pub const fn from_sequence(sequence: u64) -> Self {
        Self(sequence)
    }

    /// Sequence number of the snapshot.
    pub const fn sequence(self) -> u64 {
        self.0
    }
}

impl fmt::Display for SnapshotId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "snapshot#{}", self.0)
    }
}

/// Lifecycle failures outside metered execution. No receipt is issued.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EngineError {
    /// The candidate or expectation names a base that is no longer head.
    StaleHead {
        /// The base the caller expected.
        expected: SnapshotId,
        /// The current head.
        head: SnapshotId,
    },
    /// The candidate was sealed by a different engine instance.
    ForeignCandidate,
    /// No such snapshot has been published.
    UnknownSnapshot(SnapshotId),
    /// The snapshot is below the durable engine's retention floor: its state
    /// history is declared pruned and is never served, whether or not its
    /// objects are still stored (see `durable::Retention`).
    HistoryPruned {
        /// The snapshot the caller asked for.
        requested: SnapshotId,
        /// The lowest snapshot still served.
        floor: SnapshotId,
    },
    /// The checkpoint belongs to another branch or was already rolled past.
    InvalidCheckpoint,
    /// A pricing schedule must carry a larger ID than the one it replaces.
    StaleSchedule {
        /// The ID of the schedule currently committed in the candidate.
        current: u32,
    },
    /// The branch saw a storage error and refuses every further operation,
    /// checkpoint, rollback, pricing change and seal; the host must discard
    /// it (see [Fallible surface](self#fallible-surface)).
    Poisoned,
}

impl fmt::Display for EngineError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::StaleHead { expected, head } => {
                write!(formatter, "stale head: expected {expected}, head is {head}")
            }
            Self::ForeignCandidate => write!(formatter, "candidate from another engine"),
            Self::UnknownSnapshot(id) => write!(formatter, "unknown {id}"),
            Self::HistoryPruned { requested, floor } => write!(
                formatter,
                "history pruned: requested {}, floor {}",
                requested.sequence(),
                floor.sequence()
            ),
            Self::InvalidCheckpoint => write!(formatter, "invalid checkpoint"),
            Self::StaleSchedule { current } => {
                write!(formatter, "schedule id must exceed {current}")
            }
            Self::Poisoned => write!(formatter, "branch poisoned by a storage error"),
        }
    }
}

impl std::error::Error for EngineError {}

/// The complete logical state of one candidate or snapshot.
///
/// Cloning is O(1): the catalog and the host map are structurally shared
/// maps and the pricing schedule is shared. This is what checkpoints,
/// per-operation rollback and estimation copy.
#[derive(Clone, Debug)]
pub(crate) struct WorkingState {
    pub(crate) catalog: Catalog,
    pub(crate) next_namespace: u64,
    pub(crate) pricing: Arc<PriceSchedule>,
    /// Host-owned entries, outside every namespace and every data-plane path.
    pub(crate) host: HostMap,
}

impl WorkingState {
    /// Resolves a namespace for an unmetered statistic.
    fn namespace(
        &self,
        nodes: &Nodes,
        namespace: NamespaceId,
    ) -> Result<Option<NamespaceState>, StorageError> {
        let found = self
            .catalog
            .get(&namespace, &nodes.catalog(), &mut Probe::new())
            .map_err(|error| error.0)?;
        found.map(|leaf| checked(leaf, namespace)).transpose()
    }

    fn host_get(&self, nodes: &Nodes, key: &HostKey) -> Result<Option<Vec<u8>>, StorageError> {
        let found = self
            .host
            .get(key, &nodes.host(), &mut Probe::new())
            .map_err(|error| error.0)?;
        Ok(found.map(|value| value.0.to_vec()))
    }

    fn host_range(
        &self,
        nodes: &Nodes,
        from: &HostKey,
        limit: usize,
    ) -> Result<Vec<(HostKey, Vec<u8>)>, StorageError> {
        let loader = nodes.host();
        self.host
            .range_from(from, &loader)
            .take(limit)
            .map(|entry| {
                entry
                    .map(|(key, value)| (key, value.0.to_vec()))
                    .map_err(|error| error.0)
            })
            .collect()
    }

    /// The global part of the state; see [`StateHeader`]. Hashes whatever
    /// map nodes are not hashed yet and reads nothing.
    pub(crate) fn header(&self) -> StateHeader {
        StateHeader {
            format: STATE_FORMAT,
            catalog_root: self.catalog.root_hash(),
            catalog_len: self.catalog.len(),
            next_namespace: self.next_namespace,
            pricing: PriceSchedule::clone(&self.pricing),
            host_root: self.host.root_hash(),
            host_len: self.host.len(),
        }
    }
}

/// Checks that a catalog leaf read under `namespace` describes it.
fn checked(leaf: NamespaceState, namespace: NamespaceId) -> Result<NamespaceState, StorageError> {
    if leaf.descriptor.id == namespace {
        Ok(leaf)
    } else {
        Err(corrupt(format!(
            "the catalog leaf of {namespace} describes {}",
            leaf.descriptor.id
        )))
    }
}

#[derive(Debug)]
struct SnapshotState {
    id: SnapshotId,
    base: Option<SnapshotId>,
    instance: u64,
    limits: Limits,
    state: WorkingState,
    /// Where the state's committed nodes are read from.
    nodes: Nodes,
    /// The state root, computed on first request.
    root: OnceLock<B256>,
    /// The durable engine's retention pin, released with the last clone.
    _pin: Option<crate::durable::retention::Pin>,
}

/// Retains every published snapshot and advances the head by publication.
#[derive(Debug)]
pub struct Engine {
    instance: u64,
    snapshots: Vec<Arc<SnapshotState>>,
    limits: Limits,
    nodes: Nodes,
}

impl Engine {
    /// Creates an engine whose genesis snapshot has no namespaces and commits
    /// `schedule` as the initial pricing, with [`Limits::DEFAULT`].
    pub fn new(schedule: PriceSchedule) -> Self {
        Self::with_limits(schedule, Limits::DEFAULT)
    }

    /// Creates an engine that admits at most `namespace_cap` namespaces, with
    /// the other limits at their defaults.
    pub fn with_namespace_cap(schedule: PriceSchedule, namespace_cap: u64) -> Self {
        Self::with_limits(
            schedule,
            Limits {
                namespace_cap,
                ..Limits::DEFAULT
            },
        )
    }

    /// Creates an engine with explicit instance parameters. They are fixed
    /// for the engine's lifetime and checked at admission.
    ///
    /// # Panics
    ///
    /// If `limits` fail [`Limits::validate`] (a cell limit above its hard
    /// cap): a configuration error, not an input error.
    pub fn with_limits(schedule: PriceSchedule, limits: Limits) -> Self {
        if let Err(reason) = limits.validate() {
            panic!("invalid engine limits: {reason}");
        }
        let state = WorkingState {
            catalog: Catalog::new(),
            next_namespace: 1,
            pricing: Arc::new(schedule),
            host: HostMap::new(),
        };
        Self::from_genesis(state, limits, Nodes::memory())
    }

    /// Test aid: an engine whose genesis snapshot is the committed state
    /// `header` describes, with every map node read lazily, on demand,
    /// through `reader`.
    ///
    /// Nothing is read here; the header is only checked (format and map
    /// handles). Every later read and write may fail with the reader's
    /// storage errors, or with `Missing`/`Corrupt` for absent or damaged
    /// nodes (see [Fallible surface](self#fallible-surface)). The objects
    /// of a state are what [`Snapshot::staged_objects`] returns for it; its
    /// genesis state root is `header.root()`.
    #[doc(hidden)]
    pub fn from_state(
        header: &StateHeader,
        limits: Limits,
        reader: Arc<dyn ObjectReader>,
    ) -> Result<Self, StorageError> {
        if header.format != STATE_FORMAT {
            return Err(StorageError::new(
                StorageErrorKind::Incompatible,
                format!("state format {} is not {STATE_FORMAT}", header.format),
            ));
        }
        let handle = |error: arkiv_db_trie::TrieError| corrupt(format!("state header: {error}"));
        let state = WorkingState {
            catalog: Catalog::from_root(header.catalog_root, header.catalog_len).map_err(handle)?,
            next_namespace: header.next_namespace,
            pricing: Arc::new(header.pricing.clone()),
            host: HostMap::from_root(header.host_root, header.host_len).map_err(handle)?,
        };
        Ok(Self::from_genesis(state, limits, Nodes::stored(reader)))
    }

    fn from_genesis(state: WorkingState, limits: Limits, nodes: Nodes) -> Self {
        let instance = NEXT_INSTANCE.fetch_add(1, Ordering::Relaxed);
        let genesis = SnapshotState {
            id: SnapshotId::GENESIS,
            base: None,
            instance,
            limits,
            state,
            nodes: nodes.clone(),
            root: OnceLock::new(),
            _pin: None,
        };
        Self {
            instance,
            snapshots: vec![Arc::new(genesis)],
            limits,
            nodes,
        }
    }

    /// The configured directory capacity.
    pub const fn namespace_cap(&self) -> u64 {
        self.limits.namespace_cap
    }

    /// The instance limits every record write and query is checked against.
    pub const fn limits(&self) -> Limits {
        self.limits
    }

    /// The most recently published snapshot.
    pub fn head(&self) -> SnapshotId {
        self.latest().id
    }

    fn latest(&self) -> &Arc<SnapshotState> {
        self.snapshots
            .last()
            .expect("an engine always has a genesis snapshot")
    }

    /// Number of retained snapshots, including genesis.
    pub fn snapshot_count(&self) -> usize {
        self.snapshots.len()
    }

    /// Pins a published snapshot for reading. Snapshots are retained for the
    /// process lifetime, so any published ID resolves.
    pub fn snapshot(&self, id: SnapshotId) -> Result<Snapshot, EngineError> {
        usize::try_from(id.0)
            .ok()
            .and_then(|index| self.snapshots.get(index))
            .map(|state| Snapshot {
                state: Arc::clone(state),
            })
            .ok_or(EngineError::UnknownSnapshot(id))
    }

    /// Pins the current head for reading.
    pub fn head_snapshot(&self) -> Snapshot {
        Snapshot {
            state: Arc::clone(self.latest()),
        }
    }

    /// Starts a candidate on top of `expected_head`, which must be the head.
    ///
    /// `block_budget` bounds the total write cost of the candidate; `None`
    /// bounds it only by the counter width. Several branches may exist at
    /// once; at most one of them can publish.
    pub fn begin(
        &self,
        expected_head: SnapshotId,
        block_budget: Option<Cost>,
    ) -> Result<Branch, EngineError> {
        let head = self.head();
        if expected_head != head {
            return Err(EngineError::StaleHead {
                expected: expected_head,
                head,
            });
        }
        Ok(self.head_snapshot().branch(0, block_budget))
    }

    /// Publishes a sealed candidate as the new head, atomically.
    ///
    /// The candidate's base must still be the head; otherwise nothing changes
    /// and the candidate is rejected. The candidate is borrowed, so a rejected
    /// candidate keeps its receipts. Publication is in-process only: it makes
    /// the state visible to later readers but does not make it durable.
    pub fn publish(&mut self, candidate: &SealedCandidate) -> Result<SnapshotId, EngineError> {
        if candidate.instance != self.instance {
            return Err(EngineError::ForeignCandidate);
        }
        let head = self.head();
        if candidate.base != head {
            return Err(EngineError::StaleHead {
                expected: candidate.base,
                head,
            });
        }
        let id = SnapshotId(u64::try_from(self.snapshots.len()).expect("snapshot count fits u64"));
        let root = candidate.root.clone();
        self.snapshots.push(Arc::new(SnapshotState {
            id,
            base: Some(candidate.base),
            instance: self.instance,
            limits: self.limits,
            state: candidate.state.clone(),
            nodes: self.nodes.clone(),
            root,
            _pin: None,
        }));
        Ok(id)
    }
}

/// A frozen candidate: no further operations, ready for publication.
///
/// Cloning is O(1) apart from the receipt list.
#[derive(Clone, Debug)]
pub struct SealedCandidate {
    pub(crate) instance: u64,
    pub(crate) generation: u64,
    base: SnapshotId,
    pub(crate) state: WorkingState,
    receipts: Vec<Receipt>,
    spent: Cost,
    root: OnceLock<B256>,
}

impl SealedCandidate {
    /// The snapshot the candidate must be published on top of.
    pub const fn base(&self) -> SnapshotId {
        self.base
    }

    /// Write receipts in execution order, including rejected operations.
    pub fn receipts(&self) -> &[Receipt] {
        &self.receipts
    }

    /// Aggregate write cost of the candidate: the sum of its receipts.
    pub const fn spent(&self) -> Cost {
        self.spent
    }

    /// The global part of the candidate's state; its digest is
    /// [`state_root`](Self::state_root). Hashes only the map nodes the
    /// candidate built that are not hashed yet; never reads storage.
    pub fn state_header(&self) -> StateHeader {
        self.state.header()
    }

    /// The candidate's state root, `digest("arkiv/state/v1",
    /// encode(state_header))`: the root its published snapshot reports.
    /// Computed on first request and kept.
    pub fn state_root(&self) -> B256 {
        *self.root.get_or_init(|| self.state.header().root())
    }

    /// Test aid: see [`Snapshot::staged_objects`].
    #[doc(hidden)]
    pub fn staged_objects(&self) -> Vec<StagedObject> {
        self.state.staging().objects
    }

    /// Test aid: per map, the nodes of this state that exist only in memory
    /// and carry a hash, which computing the state root hashes once each.
    /// They are counted where the staging walk meets them, before
    /// [`staged_objects`](Self::staged_objects) keeps each `(space, map,
    /// id)` once, so each map's count is at least its staged objects.
    #[doc(hidden)]
    pub fn hashed_nodes(&self) -> BTreeMap<MapKind, u64> {
        self.state.staging().recorded
    }
}
