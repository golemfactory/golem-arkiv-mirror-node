use std::{
    ops::Bound,
    sync::{Arc, OnceLock},
};

use arkiv_db_storage::{Entry, versioned::StorageError};
use arkiv_db_trie::Probe;

use crate::{
    codec::B256,
    layout::{Catalog, HostMap},
    meter::{Budget, PriceSchedule},
    namespace::{NamespaceDescriptor, NamespaceId},
    nodes::{Nodes, corrupt},
    query::{Cursor, CursorContext, Projection, Query, QueryPage},
    record::{CellName, Limits, RecordId, RecordKey, Row, TypeTag, Value},
    state::{NamespaceRoots, StagedObject, StateHeader},
    typed,
};

use super::{
    HostKey, SnapshotId, SnapshotState, WorkingState, count, in_memory,
    ops::ReadResult,
    read::{QueryRequest, View},
};

/// A read handle that pins one published state.
///
/// Every read consults only this snapshot's catalog and record maps. Later
/// publications, transfers and discarded candidates cannot change its
/// answers, and keys deleted at a later head remain visible here.
#[derive(Clone, Debug)]
pub struct Snapshot {
    pub(super) state: Arc<SnapshotState>,
}

impl Snapshot {
    pub(crate) fn from_header(
        id: SnapshotId,
        base: Option<SnapshotId>,
        instance: u64,
        limits: Limits,
        header: &StateHeader,
        nodes: Nodes,
        pin: Option<crate::durable::retention::Pin>,
    ) -> Result<Self, StorageError> {
        // Strict decoding checks allocator/handle relations as well as format.
        let header: StateHeader =
            crate::decode_exact(&crate::encode(header)).map_err(|e| corrupt(e.to_string()))?;
        let state = WorkingState {
            catalog: Catalog::from_root(header.catalog_root, header.catalog_len)
                .map_err(|e| corrupt(e.to_string()))?,
            next_namespace: header.next_namespace,
            pricing: Arc::new(header.pricing.clone()),
            host: HostMap::from_root(header.host_root, header.host_len)
                .map_err(|e| corrupt(e.to_string()))?,
        };
        let root = OnceLock::from(header.root());
        Ok(Self {
            state: Arc::new(SnapshotState {
                id,
                base,
                instance,
                limits,
                state,
                nodes,
                root,
                _pin: pin,
            }),
        })
    }

    /// Produces bounded complete equality evidence for this immutable
    /// state. The host must resolve the request's block identity to this
    /// snapshot; this method independently checks its state root.
    pub fn prove_eq(
        &self,
        request: &crate::proof::EqRequest,
        limits: crate::proof::ProofLimits,
        budget: Budget,
    ) -> Result<crate::proof::GeneratedProof, crate::proof::ProofError> {
        crate::proof::generate(
            &self.state.state.catalog,
            &self.state.nodes,
            self.state_header(),
            self.state.limits,
            request,
            limits,
            budget,
        )
    }

    /// Complete index-backed ordered range, with no pagination or truncation.
    /// All matching terms, postings and rows must fit the caller's proof caps.
    pub fn prove_range(
        &self,
        request: &crate::proof::RangeRequest,
        limits: crate::proof::RangeProofLimits,
        budget: Budget,
    ) -> Result<crate::proof::GeneratedProof, crate::proof::ProofError> {
        crate::proof::generate_range(
            &self.state.state.catalog,
            &self.state.nodes,
            self.state_header(),
            self.state.limits,
            request,
            limits,
            budget,
        )
    }

    /// Bounded host-state membership/nonmembership evidence. Host-internal API;
    /// callers decide which host keys are public.
    pub fn prove_host(&self, key: &HostKey) -> Result<Vec<u8>, StorageError> {
        let path = self
            .state
            .state
            .host
            .prove(key, &self.state.nodes.host(), &mut Probe::new())
            .map_err(|e| e.0)?;
        let mut e = crate::Encoder::new();
        e.u32(1);
        e.put(&self.state_header());
        e.u32(path.len() as u32);
        for node in path {
            e.bytes(&node);
        }
        let bytes = e.into_bytes();
        if bytes.len() > crate::proof::HOST_PROOF_CAP {
            return Err(StorageError::new(
                crate::StorageErrorKind::Corrupt,
                "host proof exceeds structural bound",
            ));
        }
        Ok(bytes)
    }

    /// Identity of the pinned snapshot.
    pub fn id(&self) -> SnapshotId {
        self.state.id
    }

    /// The snapshot this one was published on top of; `None` for genesis.
    pub fn base(&self) -> Option<SnapshotId> {
        self.state.base
    }

    /// The pricing schedule committed in this snapshot. Reads at this
    /// snapshot are priced with it.
    pub fn pricing(&self) -> &PriceSchedule {
        &self.state.state.pricing
    }

    /// The global part of this snapshot's state: catalog and host roots
    /// with their entry counts, the namespace allocator and the pricing.
    /// Its digest is [`state_root`](Self::state_root). Hashes only map nodes
    /// not hashed yet; never reads storage, so it cannot fail.
    pub fn state_header(&self) -> StateHeader {
        self.state.state.header()
    }

    /// The state root, `digest("arkiv/state/v1", encode(state_header))`
    /// (see [`crate::state`]). It depends only on the logical content of
    /// the state, never on snapshot ids, physical node ids or cache state.
    /// Computed on first request and kept.
    pub fn state_root(&self) -> B256 {
        *self
            .state
            .root
            .get_or_init(|| self.state.state.header().root())
    }

    /// Test aid: the objects of this state that exist only in memory, each
    /// once per space and map. Every map is walked from its root, stopping
    /// at nodes read from storage; the catalog's nested namespace maps and
    /// the term leaves' posting maps are staged into their namespace's
    /// space. For a memory engine that is every object of the state; for an
    /// engine from [`Engine::from_state`], the objects its blocks created.
    /// Committing them lets [`Engine::from_state`] reopen the state from
    /// its header.
    #[doc(hidden)]
    pub fn staged_objects(&self) -> Vec<StagedObject> {
        self.state.state.staging().objects
    }

    /// Test aid: the roots of `namespace`'s maps and its record allocator,
    /// as its catalog leaf records them; `None` if it does not exist.
    #[doc(hidden)]
    pub fn try_namespace_roots(
        &self,
        namespace: NamespaceId,
    ) -> Result<Option<NamespaceRoots>, StorageError> {
        Ok(self
            .state
            .state
            .namespace(&self.state.nodes, namespace)?
            .map(|leaf| leaf.roots()))
    }

    /// Number of namespaces in the catalog. An O(1) statistic, not a metered
    /// data-plane read.
    pub fn try_namespace_count(&self) -> Result<usize, StorageError> {
        Ok(count(self.state.state.catalog.len()))
    }

    /// Number of records in a namespace, if it exists. A statistic that
    /// resolves the namespace in the catalog, not a metered data-plane read.
    pub fn try_record_count(&self, namespace: NamespaceId) -> Result<Option<usize>, StorageError> {
        Ok(self
            .state
            .state
            .namespace(&self.state.nodes, namespace)?
            .map(|state| count(state.records.len())))
    }

    /// Whether `namespace`'s raw record map has the same root in both
    /// snapshots, hence the same contents. For a non-empty namespace this
    /// shows that no record changed between the two states; the check
    /// compares shared nodes or stored ids first and hashes only nodes not
    /// hashed yet. Two empty record maps always count as shared, and
    /// snapshots of different engines are compared by ordinal only.
    pub fn try_records_shared_with(
        &self,
        other: &Self,
        namespace: NamespaceId,
    ) -> Result<bool, StorageError> {
        Ok(
            match (
                self.state.state.namespace(&self.state.nodes, namespace)?,
                other.state.state.namespace(&other.state.nodes, namespace)?,
            ) {
                (Some(left), Some(right)) => left.records.same_root(&right.records),
                _ => false,
            },
        )
    }

    /// Number of live typed records in a namespace, if it exists. A
    /// statistic, not a metered data-plane read.
    pub fn try_typed_record_count(
        &self,
        namespace: NamespaceId,
    ) -> Result<Option<usize>, StorageError> {
        Ok(self
            .state
            .state
            .namespace(&self.state.nodes, namespace)?
            .map(|state| count(state.typed.record_count())))
    }

    /// Number of index terms (value and existence) in a namespace, if it
    /// exists. A statistic, not a metered data-plane read.
    pub fn try_term_count(&self, namespace: NamespaceId) -> Result<Option<usize>, StorageError> {
        Ok(self
            .state
            .state
            .namespace(&self.state.nodes, namespace)?
            .map(|state| count(state.typed.term_count())))
    }

    /// Whether `namespace`'s typed rows, key bindings and index have the
    /// same roots in both snapshots (see
    /// [`try_records_shared_with`](Self::try_records_shared_with)).
    pub fn try_typed_shared_with(
        &self,
        other: &Self,
        namespace: NamespaceId,
    ) -> Result<bool, StorageError> {
        Ok(
            match (
                self.state.state.namespace(&self.state.nodes, namespace)?,
                other.state.state.namespace(&other.state.nodes, namespace)?,
            ) {
                (Some(left), Some(right)) => left.typed.shares_with(&right.typed),
                _ => false,
            },
        )
    }

    /// Test aid: whether `namespace`'s key bindings and index terms are
    /// exactly what its live rows imply. O(rows × cells) and unmetered; not
    /// a data-plane read.
    #[doc(hidden)]
    pub fn try_index_consistent(
        &self,
        namespace: NamespaceId,
    ) -> Result<Option<bool>, StorageError> {
        index_consistent(&self.state.state, &self.state.nodes, namespace)
    }

    fn view(&self) -> View<'_> {
        View {
            state: &self.state.state,
            nodes: &self.state.nodes,
            pricing: &self.state.state.pricing,
            limits: &self.state.limits,
        }
    }

    /// Returns the value of `key` in `namespace`, metered as a read.
    /// Keys longer than [`Limits::MAX_RAW_KEY_LEN`] fail admission.
    pub fn try_get(
        &self,
        namespace: NamespaceId,
        key: &[u8],
        budget: Budget,
    ) -> Result<ReadResult<Option<Vec<u8>>>, StorageError> {
        self.view().get(namespace, key, budget)
    }

    /// Returns the typed record bound to `key`, projected, metered as a read.
    pub fn try_get_record(
        &self,
        namespace: NamespaceId,
        key: &RecordKey,
        projection: &Projection,
        budget: Budget,
    ) -> Result<ReadResult<Option<Row>>, StorageError> {
        self.view().record(namespace, key, projection, budget)
    }

    /// Counts every live record matching `query`, metered as a read: the
    /// same enumeration as a complete page walk with no row emitted.
    pub fn try_count(
        &self,
        namespace: NamespaceId,
        query: &Query,
        budget: Budget,
    ) -> Result<ReadResult<u64>, StorageError> {
        self.view().count(namespace, query, budget)
    }

    /// The distinct indexed values of attribute `name` with type `tag` held
    /// by live records, in value order from `lower`, at most `limit` of
    /// them, metered as a read. Positional: only the returned terms and the
    /// seek are charged.
    pub fn try_terms(
        &self,
        namespace: NamespaceId,
        name: &CellName,
        tag: TypeTag,
        lower: Bound<Value>,
        limit: usize,
        budget: Budget,
    ) -> Result<ReadResult<Vec<Value>>, StorageError> {
        self.view()
            .terms(namespace, name, tag, &lower, limit, budget)
    }

    /// Evaluates `query` against this snapshot and returns one page of at
    /// most `limit` rows after `cursor`, metered as a read.
    ///
    /// Evaluation is lazy and positioned after the cursor: the charge covers
    /// the candidates scanned to fill the page plus each group's first match
    /// beyond it, which makes `next` an exact claim. The cursor must have
    /// been issued by this snapshot for the same namespace, query and
    /// projection.
    pub fn try_query(
        &self,
        namespace: NamespaceId,
        query: &Query,
        projection: &Projection,
        limit: usize,
        cursor: Option<&Cursor>,
        budget: Budget,
    ) -> Result<ReadResult<QueryPage>, StorageError> {
        let request = QueryRequest {
            namespace,
            query,
            projection,
            limit,
            cursor,
            context: CursorContext::Snapshot {
                instance: self.state.instance,
                id: self.state.id,
            },
        };
        self.view().query(&request, budget)
    }

    /// A cursor of this snapshot that continues `query` with `projection`
    /// after record `after`, as if a page had ended there.
    ///
    /// A cursor is a position, never a count, so any record id is a valid
    /// one: the next page starts at the first match with a larger id. This
    /// lets a reader that walked the matches with a cheaper projection (ids
    /// only, say) read on from where it stopped with the one it returns.
    /// Unmetered: it reads no state.
    pub fn cursor_after(
        &self,
        namespace: NamespaceId,
        query: &Query,
        projection: &Projection,
        after: RecordId,
    ) -> Cursor {
        Cursor {
            context: CursorContext::Snapshot {
                instance: self.state.instance,
                id: self.state.id,
            },
            namespace,
            query: Arc::new(query.clone()),
            projection: Arc::new(projection.clone()),
            after,
        }
    }

    /// Returns `namespace`'s entries starting with `prefix`, in key order,
    /// metered as a read. An empty prefix scans the whole namespace.
    /// Prefixes longer than [`Limits::MAX_RAW_KEY_LEN`] fail admission.
    pub fn try_scan_prefix(
        &self,
        namespace: NamespaceId,
        prefix: &[u8],
        budget: Budget,
    ) -> Result<ReadResult<Vec<Entry>>, StorageError> {
        self.view().scan(namespace, prefix, budget)
    }

    /// Returns the namespace descriptor as of this snapshot, metered as a read.
    pub fn try_descriptor(
        &self,
        namespace: NamespaceId,
        budget: Budget,
    ) -> Result<ReadResult<NamespaceDescriptor>, StorageError> {
        self.view().descriptor(namespace, budget)
    }

    /// The host-state value under `key` in this snapshot. Unmetered and
    /// host-internal; see [Host state](super#host-state).
    pub fn try_host_get(&self, key: &HostKey) -> Result<Option<Vec<u8>>, StorageError> {
        self.state.state.host_get(&self.state.nodes, key)
    }

    /// At most `limit` host-state entries of this snapshot whose keys are at
    /// least `from`, in key order. Unmetered and host-internal; the work is
    /// bounded by `limit`.
    pub fn try_host_range(
        &self,
        from: &HostKey,
        limit: usize,
    ) -> Result<Vec<(HostKey, Vec<u8>)>, StorageError> {
        self.state.state.host_range(&self.state.nodes, from, limit)
    }
}

/// Infallible forms of the `try_` methods, for an engine whose state is
/// entirely in memory. Each panics on a storage error; see
/// [Fallible surface](super#fallible-surface).
impl Snapshot {
    pub fn namespace_count(&self) -> usize {
        in_memory(self.try_namespace_count())
    }

    pub fn record_count(&self, namespace: NamespaceId) -> Option<usize> {
        in_memory(self.try_record_count(namespace))
    }

    pub fn records_shared_with(&self, other: &Self, namespace: NamespaceId) -> bool {
        in_memory(self.try_records_shared_with(other, namespace))
    }

    pub fn typed_record_count(&self, namespace: NamespaceId) -> Option<usize> {
        in_memory(self.try_typed_record_count(namespace))
    }

    pub fn term_count(&self, namespace: NamespaceId) -> Option<usize> {
        in_memory(self.try_term_count(namespace))
    }

    pub fn typed_shared_with(&self, other: &Self, namespace: NamespaceId) -> bool {
        in_memory(self.try_typed_shared_with(other, namespace))
    }

    #[doc(hidden)]
    pub fn index_consistent(&self, namespace: NamespaceId) -> Option<bool> {
        in_memory(self.try_index_consistent(namespace))
    }

    pub fn get(
        &self,
        namespace: NamespaceId,
        key: &[u8],
        budget: Budget,
    ) -> ReadResult<Option<Vec<u8>>> {
        in_memory(self.try_get(namespace, key, budget))
    }

    pub fn get_record(
        &self,
        namespace: NamespaceId,
        key: &RecordKey,
        projection: &Projection,
        budget: Budget,
    ) -> ReadResult<Option<Row>> {
        in_memory(self.try_get_record(namespace, key, projection, budget))
    }

    pub fn count(&self, namespace: NamespaceId, query: &Query, budget: Budget) -> ReadResult<u64> {
        in_memory(self.try_count(namespace, query, budget))
    }

    pub fn terms(
        &self,
        namespace: NamespaceId,
        name: &CellName,
        tag: TypeTag,
        lower: Bound<Value>,
        limit: usize,
        budget: Budget,
    ) -> ReadResult<Vec<Value>> {
        in_memory(self.try_terms(namespace, name, tag, lower, limit, budget))
    }

    pub fn query(
        &self,
        namespace: NamespaceId,
        query: &Query,
        projection: &Projection,
        limit: usize,
        cursor: Option<&Cursor>,
        budget: Budget,
    ) -> ReadResult<QueryPage> {
        in_memory(self.try_query(namespace, query, projection, limit, cursor, budget))
    }

    pub fn scan_prefix(
        &self,
        namespace: NamespaceId,
        prefix: &[u8],
        budget: Budget,
    ) -> ReadResult<Vec<Entry>> {
        in_memory(self.try_scan_prefix(namespace, prefix, budget))
    }

    pub fn descriptor(
        &self,
        namespace: NamespaceId,
        budget: Budget,
    ) -> ReadResult<NamespaceDescriptor> {
        in_memory(self.try_descriptor(namespace, budget))
    }
}

/// See [`Snapshot::try_index_consistent`].
pub(super) fn index_consistent(
    state: &WorkingState,
    nodes: &Nodes,
    namespace: NamespaceId,
) -> Result<Option<bool>, StorageError> {
    let Some(leaf) = state.namespace(nodes, namespace)? else {
        return Ok(None);
    };
    let loaders = nodes.namespace(namespace);
    leaf.typed
        .is_consistent(&loaders)
        .map(Some)
        .map_err(typed::storage)
}
