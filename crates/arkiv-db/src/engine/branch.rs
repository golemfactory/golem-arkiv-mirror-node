use std::{
    ops::{Bound, Range},
    sync::{Arc, OnceLock, atomic::Ordering},
};

use arkiv_db_storage::{Entry, versioned::StorageError};
use arkiv_db_trie::Probe;

use crate::{
    codec::B256,
    layout::Blob,
    meter::{
        Budget, Cost, Meter, Outcome, PhysicalTrace, PriceSchedule, Receipt, ReceiptKind, Rejection,
    },
    namespace::{NamespaceDescriptor, NamespaceId, OwnerId},
    nodes::{Fault, Nodes},
    query::{Cursor, CursorContext, Projection, Query, QueryPage},
    record::{CellName, Limits, RecordKey, Row, TypeTag, Value},
};

use super::{
    EngineError, HOST_VALUE_MAX, HostKey, HostStateError, NEXT_BRANCH_TOKEN, SealedCandidate,
    SnapshotId, SnapshotState, WorkingState, count, in_memory,
    ops::{Authority, Executed, ReadResult, Rejected, WriteOp, WriteResult},
    read::{QueryRequest, View},
    snapshot::{Snapshot, index_consistent},
    write::{admit, apply},
};

/// A checkpoint inside one branch; see [`Branch::checkpoint`].
///
/// The branch token is process-wide unique and the serial is per branch, so a
/// checkpoint is accepted only by the branch that issued it and only while it
/// has not been rolled past.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Checkpoint {
    token: u64,
    serial: u64,
    index: usize,
}

#[derive(Debug)]
struct Saved {
    serial: u64,
    state: WorkingState,
    receipt_count: usize,
}

impl Snapshot {
    pub(crate) fn branch(&self, generation: u64, block_budget: Option<Cost>) -> Branch {
        let base = Arc::clone(&self.state);
        Branch {
            instance: base.instance,
            generation,
            token: NEXT_BRANCH_TOKEN.fetch_add(1, Ordering::Relaxed),
            limits: base.limits,
            mutations: 0,
            working: base.state.clone(),
            pricing: Arc::clone(&base.state.pricing),
            base,
            checkpoints: Vec::new(),
            next_serial: 1,
            receipts: Vec::new(),
            block_limit: block_budget.unwrap_or(Cost::new(u64::MAX)),
            spent: Cost::ZERO,
            poison: OnceLock::new(),
        }
    }
}

/// A candidate block under construction on top of one base snapshot.
///
/// Operations execute in call order against the branch's working state and
/// are priced with the schedule committed in the base snapshot. The branch
/// owns its base, so it stays valid while other candidates are published;
/// it can then no longer be published itself.
///
/// A storage error during any branch call poisons the branch; see
/// [Fallible surface](super#fallible-surface).
#[derive(Debug)]
pub struct Branch {
    instance: u64,
    generation: u64,
    token: u64,
    limits: Limits,
    /// Serial of applied state changes; branch cursors are bound to it.
    mutations: u64,
    base: Arc<SnapshotState>,
    working: WorkingState,
    pricing: Arc<PriceSchedule>,
    checkpoints: Vec<Saved>,
    next_serial: u64,
    receipts: Vec<Receipt>,
    block_limit: Cost,
    spent: Cost,
    /// The first storage error the branch saw; set once, never cleared.
    poison: OnceLock<StorageError>,
}

impl Branch {
    /// The snapshot this candidate is built on.
    pub fn base(&self) -> SnapshotId {
        self.base.id
    }

    /// The pricing pinned for every write, estimate and read in this branch.
    pub fn pricing(&self) -> &PriceSchedule {
        &self.pricing
    }

    /// The block's total write budget.
    pub const fn block_limit(&self) -> Cost {
        self.block_limit
    }

    /// The engine's instance limits.
    pub const fn limits(&self) -> Limits {
        self.limits
    }

    /// Units charged to write receipts so far, including rejected operations.
    pub const fn spent(&self) -> Cost {
        self.spent
    }

    /// Block budget still available to writes.
    pub const fn remaining(&self) -> Cost {
        self.block_limit.saturating_sub(self.spent)
    }

    /// Write receipts in execution order, including rejected operations.
    /// Admission rejections, estimates and reads are never recorded here.
    pub fn receipts(&self) -> &[Receipt] {
        &self.receipts
    }

    /// Whether a storage error has poisoned the branch; see
    /// [Fallible surface](super#fallible-surface).
    pub fn is_poisoned(&self) -> bool {
        self.poison.get().is_some()
    }

    fn nodes(&self) -> &Nodes {
        &self.base.nodes
    }

    /// Refuses every call on a poisoned branch with the poisoning error.
    fn guard(&self) -> Result<(), StorageError> {
        match self.poison.get() {
            None => Ok(()),
            Some(error) => Err(StorageError::new(
                error.kind,
                format!(
                    "branch poisoned by an earlier storage error: {}",
                    error.detail
                ),
            )),
        }
    }

    /// Poisons the branch with its first storage error and returns `error`.
    fn fail(&self, error: StorageError) -> StorageError {
        let _ = self.poison.set(error.clone());
        error
    }

    /// Runs a call that changes nothing on the working state: refused when
    /// poisoned, and poisoning on a storage error.
    fn read<T>(
        &self,
        call: impl FnOnce(&WorkingState, &Nodes) -> Result<T, StorageError>,
    ) -> Result<T, StorageError> {
        self.guard()?;
        call(&self.working, self.nodes()).map_err(|error| self.fail(error))
    }

    /// Runs a metered read of the working state at the pinned pricing, under
    /// the rules of [`read`](Self::read).
    fn metered<T>(
        &self,
        call: impl FnOnce(View<'_>) -> Result<T, StorageError>,
    ) -> Result<T, StorageError> {
        self.read(|state, nodes| {
            call(View {
                state,
                nodes,
                pricing: &self.pricing,
                limits: &self.limits,
            })
        })
    }

    /// Number of namespaces in the branch's current state. A statistic, not
    /// a metered read.
    pub fn try_namespace_count(&self) -> Result<usize, StorageError> {
        self.read(|state, _| Ok(count(state.catalog.len())))
    }

    /// Number of records in a namespace at the branch's current state, if it
    /// exists. A statistic, not a metered read.
    pub fn try_record_count(&self, namespace: NamespaceId) -> Result<Option<usize>, StorageError> {
        self.read(|state, nodes| {
            Ok(state
                .namespace(nodes, namespace)?
                .map(|state| count(state.records.len())))
        })
    }

    /// Number of live typed records in a namespace at the branch's current
    /// state, if it exists. A statistic, not a metered read.
    pub fn try_typed_record_count(
        &self,
        namespace: NamespaceId,
    ) -> Result<Option<usize>, StorageError> {
        self.read(|state, nodes| {
            Ok(state
                .namespace(nodes, namespace)?
                .map(|state| count(state.typed.record_count())))
        })
    }

    /// Number of index terms in a namespace at the branch's current state,
    /// if it exists. A statistic, not a metered read.
    pub fn try_term_count(&self, namespace: NamespaceId) -> Result<Option<usize>, StorageError> {
        self.read(|state, nodes| {
            Ok(state
                .namespace(nodes, namespace)?
                .map(|state| count(state.typed.term_count())))
        })
    }

    /// Test aid: see [`Snapshot::index_consistent`].
    #[doc(hidden)]
    pub fn try_index_consistent(
        &self,
        namespace: NamespaceId,
    ) -> Result<Option<bool>, StorageError> {
        self.read(|state, nodes| index_consistent(state, nodes, namespace))
    }

    /// Executes one write at the current position on behalf of `actor`.
    ///
    /// Admission: malformed arguments are rejected with
    /// [`Rejection::InvalidArgument`] and a limited budget larger than the
    /// block's remaining budget with [`Rejection::ExceedsBlockBudget`], both
    /// before any charge and without recording anything. Otherwise the
    /// effective limit is the budget, or the block's remaining budget when
    /// the operation is unlimited. Charges precede the work they cover. On
    /// rejection the operation's changes are discarded, but its receipt is
    /// recorded and its spent units count against the block.
    ///
    /// A storage error restores the state from before the operation, records
    /// nothing and poisons the branch.
    pub fn try_execute(
        &mut self,
        actor: OwnerId,
        op: WriteOp,
        budget: Budget,
    ) -> Result<WriteResult, StorageError> {
        self.execute_as(Authority::Owner(actor), op, budget)
    }

    /// Executes one typed record write with the host's own authority: the
    /// namespace owner check is skipped, everything else (validation,
    /// admission, charges, rollback, receipts) is identical to
    /// [`try_execute`](Self::try_execute). Namespace operations and raw byte writes
    /// are refused at admission with
    /// [`crate::ArgumentFault::NotHostOperation`], so descriptors are never
    /// host-writable.
    ///
    /// This is the trusted path for chain-driven transitions such as expiry
    /// deletion; a host must never expose it to user requests.
    ///
    /// A storage error behaves as in [`try_execute`](Self::try_execute).
    pub fn try_execute_host(
        &mut self,
        op: WriteOp,
        budget: Budget,
    ) -> Result<WriteResult, StorageError> {
        self.execute_as(Authority::Host, op, budget)
    }

    /// Charges deterministic host work against the same block budget as writes.
    /// This trusted API records a separate receipt; rollback retains its charge.
    /// Host work has no engine counting ledger. An insufficient budget charges zero.
    pub fn try_charge_host(
        &mut self,
        units: u64,
        budget: Budget,
    ) -> Result<Result<Receipt, Rejected>, StorageError> {
        self.guard()?;
        let reason = if budget
            .limit_units()
            .is_some_and(|limit| limit > self.remaining())
        {
            Some(Rejection::ExceedsBlockBudget {
                remaining: self.remaining(),
            })
        } else if Cost::new(units) > self.remaining()
            || budget
                .limit_units()
                .is_some_and(|limit| Cost::new(units) > limit)
        {
            Some(Rejection::OutOfBudget)
        } else {
            None
        };
        let receipt = Receipt {
            kind: ReceiptKind::Write,
            model_version: crate::MODEL_VERSION,
            schedule_id: self.pricing.id(),
            spent: if reason.is_some() {
                Cost::ZERO
            } else {
                Cost::new(units)
            },
            outcome: reason.map_or(Outcome::Applied, Outcome::Rejected),
            ledger: None,
        };
        self.spent = self
            .spent
            .checked_add(receipt.spent)
            .expect("bounded host charge");
        self.receipts.push(receipt.clone());
        Ok(match reason {
            Some(reason) => Err(Rejected {
                reason,
                receipt,
                physical: PhysicalTrace::default(),
            }),
            None => Ok(receipt),
        })
    }

    fn execute_as(
        &mut self,
        authority: Authority,
        op: WriteOp,
        budget: Budget,
    ) -> Result<WriteResult, StorageError> {
        self.guard()?;
        let remaining = self.remaining();
        if let Some(admission) = admit(
            &self.pricing,
            &self.limits,
            authority,
            &op,
            budget,
            remaining,
        ) {
            return Ok(Err(admission));
        }
        let saved = self.working.clone();
        let mut meter = Meter::new(&self.pricing, budget, Some(remaining));
        let result = apply(
            &mut self.working,
            &self.base.nodes,
            &self.limits,
            authority,
            op,
            &mut meter,
        );
        let reason = match result {
            Err(Fault::Storage(error)) => {
                // Nothing of the operation survives: no state change, no
                // receipt, no spend.
                self.working = saved;
                return Err(self.fail(error));
            }
            Err(Fault::Reject(reason)) => reason,
            Ok(effect) => {
                self.spent = self
                    .spent
                    .checked_add(meter.spent())
                    .expect("the meter never exceeds the remaining block budget");
                self.mutations += 1;
                let (receipt, physical) = meter.finish(ReceiptKind::Write, Outcome::Applied);
                self.receipts.push(receipt.clone());
                return Ok(Ok(Executed {
                    effect,
                    receipt,
                    physical,
                }));
            }
        };
        self.spent = self
            .spent
            .checked_add(meter.spent())
            .expect("the meter never exceeds the remaining block budget");
        self.working = saved;
        let (receipt, physical) = meter.finish(ReceiptKind::Write, Outcome::Rejected(reason));
        self.receipts.push(receipt.clone());
        Ok(Err(Rejected {
            reason,
            receipt,
            physical,
        }))
    }

    /// Runs `op` exactly as [`try_execute`](Self::try_execute) would, including the
    /// admission checks and the block's current remaining budget, on a
    /// disposable copy of the current state.
    ///
    /// Nothing is changed, recorded or charged to the block. The result equals
    /// execution only for the same state, operation and pricing.
    ///
    /// A storage error poisons the branch.
    pub fn try_estimate(
        &self,
        actor: OwnerId,
        op: WriteOp,
        budget: Budget,
    ) -> Result<WriteResult, StorageError> {
        self.estimate_as(Authority::Owner(actor), op, budget)
    }

    /// Estimates with the host's authority; see
    /// [`try_execute_host`](Self::try_execute_host).
    pub fn try_estimate_host(
        &self,
        op: WriteOp,
        budget: Budget,
    ) -> Result<WriteResult, StorageError> {
        self.estimate_as(Authority::Host, op, budget)
    }

    fn estimate_as(
        &self,
        authority: Authority,
        op: WriteOp,
        budget: Budget,
    ) -> Result<WriteResult, StorageError> {
        self.guard()?;
        let remaining = self.remaining();
        if let Some(admission) = admit(
            &self.pricing,
            &self.limits,
            authority,
            &op,
            budget,
            remaining,
        ) {
            return Ok(Err(admission));
        }
        let mut scratch = self.working.clone();
        let mut meter = Meter::new(&self.pricing, budget, Some(remaining));
        match apply(
            &mut scratch,
            self.nodes(),
            &self.limits,
            authority,
            op,
            &mut meter,
        ) {
            Ok(effect) => {
                let (receipt, physical) = meter.finish(ReceiptKind::Write, Outcome::Applied);
                Ok(Ok(Executed {
                    effect,
                    receipt,
                    physical,
                }))
            }
            Err(Fault::Reject(reason)) => {
                let (receipt, physical) =
                    meter.finish(ReceiptKind::Write, Outcome::Rejected(reason));
                Ok(Err(Rejected {
                    reason,
                    receipt,
                    physical,
                }))
            }
            Err(Fault::Storage(error)) => Err(self.fail(error)),
        }
    }

    /// Records the current logical state (records, descriptors, allocator,
    /// pending pricing and host state) so a transaction can be rolled back
    /// to it.
    ///
    /// # Panics
    ///
    /// If the branch is poisoned, which only an engine that reads storage
    /// can cause; such a host calls [`try_checkpoint`](Self::try_checkpoint).
    pub fn checkpoint(&mut self) -> Checkpoint {
        match self.try_checkpoint() {
            Ok(checkpoint) => checkpoint,
            Err(error) => panic!("checkpoint on a poisoned branch: {error}"),
        }
    }

    /// Fallible form of [`checkpoint`](Self::checkpoint):
    /// [`EngineError::Poisoned`] on a poisoned branch.
    pub fn try_checkpoint(&mut self) -> Result<Checkpoint, EngineError> {
        if self.is_poisoned() {
            return Err(EngineError::Poisoned);
        }
        let serial = self.next_serial;
        self.next_serial += 1;
        self.checkpoints.push(Saved {
            serial,
            state: self.working.clone(),
            receipt_count: self.receipts.len(),
        });
        Ok(Checkpoint {
            token: self.token,
            serial,
            index: self.checkpoints.len() - 1,
        })
    }

    /// Restores the state recorded by `checkpoint` and returns the index
    /// range of every receipt issued since the checkpoint, i.e. the
    /// operations whose effects are no longer in the state.
    ///
    /// Changes made after the checkpoint disappear; earlier successful work
    /// stays. Later checkpoints become invalid; this one stays usable, and
    /// rolling back to it again returns the same since-checkpoint range.
    /// Issued receipts and the block's spent units are not restored. A
    /// poisoned branch refuses with [`EngineError::Poisoned`].
    pub fn rollback_to(&mut self, checkpoint: Checkpoint) -> Result<Range<usize>, EngineError> {
        if self.is_poisoned() {
            return Err(EngineError::Poisoned);
        }
        let saved = self
            .checkpoints
            .get(checkpoint.index)
            .filter(|saved| checkpoint.token == self.token && saved.serial == checkpoint.serial)
            .ok_or(EngineError::InvalidCheckpoint)?;
        let discarded = saved.receipt_count..self.receipts.len();
        self.working = saved.state.clone();
        self.checkpoints.truncate(checkpoint.index + 1);
        self.mutations += 1;
        Ok(discarded)
    }

    /// Commits a new pricing schedule into the candidate. This is a host
    /// admin transition: it takes effect for branches based on the published
    /// result, never for this branch, whose pricing stays pinned to its base.
    /// The schedule ID must exceed the one it replaces, so receipts always
    /// name a unique pricing context. A poisoned branch refuses with
    /// [`EngineError::Poisoned`].
    pub fn set_pricing(&mut self, schedule: PriceSchedule) -> Result<(), EngineError> {
        if self.is_poisoned() {
            return Err(EngineError::Poisoned);
        }
        let current = self.working.pricing.id();
        if schedule.id() <= current {
            return Err(EngineError::StaleSchedule { current });
        }
        self.working.pricing = Arc::new(schedule);
        self.mutations += 1;
        Ok(())
    }

    /// Reads `key` from the branch's current state, metered as a read with
    /// the branch's pinned pricing. The key must fit [`Limits::MAX_RAW_KEY_LEN`].
    pub fn try_get(
        &self,
        namespace: NamespaceId,
        key: &[u8],
        budget: Budget,
    ) -> Result<ReadResult<Option<Vec<u8>>>, StorageError> {
        self.metered(|view| view.get(namespace, key, budget))
    }

    /// Scans the branch's current state, metered as a read.
    /// The prefix must fit [`Limits::MAX_RAW_KEY_LEN`].
    pub fn try_scan_prefix(
        &self,
        namespace: NamespaceId,
        prefix: &[u8],
        budget: Budget,
    ) -> Result<ReadResult<Vec<Entry>>, StorageError> {
        self.metered(|view| view.scan(namespace, prefix, budget))
    }

    /// Reads the descriptor from the branch's current state, metered as a read.
    pub fn try_descriptor(
        &self,
        namespace: NamespaceId,
        budget: Budget,
    ) -> Result<ReadResult<NamespaceDescriptor>, StorageError> {
        self.metered(|view| view.descriptor(namespace, budget))
    }

    /// Reads a typed record from the branch's current state, metered as a read.
    pub fn try_get_record(
        &self,
        namespace: NamespaceId,
        key: &RecordKey,
        projection: &Projection,
        budget: Budget,
    ) -> Result<ReadResult<Option<Row>>, StorageError> {
        self.metered(|view| view.record(namespace, key, projection, budget))
    }

    /// Counts the matches of `query` in the branch's current state; see
    /// [`Snapshot::count`].
    pub fn try_count(
        &self,
        namespace: NamespaceId,
        query: &Query,
        budget: Budget,
    ) -> Result<ReadResult<u64>, StorageError> {
        self.metered(|view| view.count(namespace, query, budget))
    }

    /// The distinct indexed values of an attribute in the branch's current
    /// state; see [`Snapshot::terms`].
    pub fn try_terms(
        &self,
        namespace: NamespaceId,
        name: &CellName,
        tag: TypeTag,
        lower: Bound<Value>,
        limit: usize,
        budget: Budget,
    ) -> Result<ReadResult<Vec<Value>>, StorageError> {
        self.metered(|view| view.terms(namespace, name, tag, &lower, limit, budget))
    }

    /// Evaluates `query` against the branch's current state; see
    /// [`Snapshot::query`]. A branch cursor is bound to this branch and its
    /// current mutation serial: any applied write, rollback, pricing change
    /// or host-state write since the cursor was issued makes it a
    /// `CursorMismatch`.
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
            context: CursorContext::Branch {
                token: self.token,
                mutations: self.mutations,
            },
        };
        self.metered(|view| view.query(&request, budget))
    }

    /// The host-state value under `key` in the branch's current state.
    /// Unmetered and host-internal; see [Host state](super#host-state).
    pub fn try_host_get(&self, key: &HostKey) -> Result<Option<Vec<u8>>, StorageError> {
        self.read(|state, nodes| state.host_get(nodes, key))
    }

    /// Binds `key` to `value` in the host-state map and returns the value it
    /// replaced. Unmetered, recorded in no receipt, and never charged to the
    /// block.
    ///
    /// A value longer than [`HOST_VALUE_MAX`] is refused with
    /// [`HostStateError::ValueTooLarge`] and changes nothing. A successful
    /// put advances the branch's mutation serial, like any applied write,
    /// even when the value is unchanged, so branch cursors issued before it
    /// become `CursorMismatch`. Checkpoints and rollbacks cover host state.
    /// A storage error changes nothing and poisons the branch.
    pub fn try_host_put(
        &mut self,
        key: HostKey,
        value: &[u8],
    ) -> Result<Result<Option<Vec<u8>>, HostStateError>, StorageError> {
        self.guard()?;
        if value.len() > HOST_VALUE_MAX {
            return Ok(Err(HostStateError::ValueTooLarge { len: value.len() }));
        }
        let inserted = self.working.host.insert(
            &key,
            Blob(Arc::from(value)),
            &self.base.nodes.host(),
            &mut Probe::new(),
        );
        let (host, previous) = inserted.map_err(|error| self.fail(error.0))?;
        self.working.host = host;
        self.mutations += 1;
        Ok(Ok(previous.map(|value| value.0.to_vec())))
    }

    /// Removes `key` from the host-state map and returns the value it held.
    /// Unmetered and host-internal. Every call advances the branch's
    /// mutation serial, like a raw delete of a missing key. A storage error
    /// changes nothing and poisons the branch.
    pub fn try_host_remove(&mut self, key: &HostKey) -> Result<Option<Vec<u8>>, StorageError> {
        self.guard()?;
        let removed = self
            .working
            .host
            .remove(key, &self.base.nodes.host(), &mut Probe::new());
        let (host, previous) = removed.map_err(|error| self.fail(error.0))?;
        self.working.host = host;
        self.mutations += 1;
        Ok(previous.map(|value| value.0.to_vec()))
    }

    /// At most `limit` host-state entries of the branch's current state whose
    /// keys are at least `from`, in key order. Unmetered and host-internal;
    /// the work is bounded by `limit`.
    pub fn try_host_range(
        &self,
        from: &HostKey,
        limit: usize,
    ) -> Result<Vec<(HostKey, Vec<u8>)>, StorageError> {
        self.read(|state, nodes| state.host_range(nodes, from, limit))
    }

    /// The state root the branch's current state would seal to, without
    /// consuming the branch: equal to the [`SealedCandidate::state_root`]
    /// of a seal right now. Hashes only the map nodes not hashed yet and
    /// keeps their hashes, so a later seal or further calls redo none of
    /// that work; never reads storage. [`EngineError::Poisoned`] on a
    /// poisoned branch.
    pub fn try_state_root(&self) -> Result<B256, EngineError> {
        if self.is_poisoned() {
            return Err(EngineError::Poisoned);
        }
        Ok(self.working.header().root())
    }

    /// Freezes the candidate. No further operations are possible.
    ///
    /// # Panics
    ///
    /// If the branch is poisoned, which only an engine that reads storage
    /// can cause; such a host calls [`try_seal`](Self::try_seal).
    pub fn seal(self) -> SealedCandidate {
        match self.try_seal() {
            Ok(sealed) => sealed,
            Err(error) => panic!("seal of a poisoned branch: {error}"),
        }
    }

    /// Fallible form of [`seal`](Self::seal): [`EngineError::Poisoned`] on
    /// a poisoned branch, which must be discarded.
    pub fn try_seal(self) -> Result<SealedCandidate, EngineError> {
        if self.is_poisoned() {
            return Err(EngineError::Poisoned);
        }
        Ok(SealedCandidate {
            instance: self.instance,
            generation: self.generation,
            base: self.base.id,
            state: self.working,
            receipts: self.receipts,
            spent: self.spent,
            root: OnceLock::new(),
        })
    }
}

/// Infallible forms of the `try_` methods, for an engine whose state is
/// entirely in memory. Each panics on a storage error; see
/// [Fallible surface](super#fallible-surface).
impl Branch {
    pub fn namespace_count(&self) -> usize {
        in_memory(self.try_namespace_count())
    }

    pub fn record_count(&self, namespace: NamespaceId) -> Option<usize> {
        in_memory(self.try_record_count(namespace))
    }

    pub fn typed_record_count(&self, namespace: NamespaceId) -> Option<usize> {
        in_memory(self.try_typed_record_count(namespace))
    }

    pub fn term_count(&self, namespace: NamespaceId) -> Option<usize> {
        in_memory(self.try_term_count(namespace))
    }

    #[doc(hidden)]
    pub fn index_consistent(&self, namespace: NamespaceId) -> Option<bool> {
        in_memory(self.try_index_consistent(namespace))
    }

    pub fn execute(&mut self, actor: OwnerId, op: WriteOp, budget: Budget) -> WriteResult {
        in_memory(self.try_execute(actor, op, budget))
    }

    pub fn execute_host(&mut self, op: WriteOp, budget: Budget) -> WriteResult {
        in_memory(self.try_execute_host(op, budget))
    }

    pub fn estimate(&self, actor: OwnerId, op: WriteOp, budget: Budget) -> WriteResult {
        in_memory(self.try_estimate(actor, op, budget))
    }

    pub fn estimate_host(&self, op: WriteOp, budget: Budget) -> WriteResult {
        in_memory(self.try_estimate_host(op, budget))
    }

    pub fn get(
        &self,
        namespace: NamespaceId,
        key: &[u8],
        budget: Budget,
    ) -> ReadResult<Option<Vec<u8>>> {
        in_memory(self.try_get(namespace, key, budget))
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
}

#[cfg(test)]
mod tests {
    //! What the integration tests cannot see: after a storage error the
    //! branch refuses every call, so only here can the test check that the
    //! failed operation left the working state exactly as it was.

    use std::{
        collections::HashMap,
        sync::atomic::{AtomicU64, Ordering},
    };

    use arkiv_db_storage::versioned::{ObjectId, ObjectReader, SpaceId, StorageErrorKind};

    use super::*;
    use crate::{engine::Engine, namespace::NamespaceName, record::Cell};

    /// Committed objects, failing every read after `allowance` reads.
    struct Reader {
        objects: HashMap<(SpaceId, ObjectId), Vec<u8>>,
        allowance: AtomicU64,
        reads: AtomicU64,
    }

    impl ObjectReader for Reader {
        fn read_object(
            &self,
            space: SpaceId,
            id: &ObjectId,
        ) -> Result<Option<Vec<u8>>, StorageError> {
            self.reads.fetch_add(1, Ordering::SeqCst);
            let left = self.allowance.load(Ordering::SeqCst);
            if left == 0 {
                return Err(StorageError::new(StorageErrorKind::Io, "injected"));
            }
            self.allowance.store(left - 1, Ordering::SeqCst);
            Ok(self.objects.get(&(space, *id)).cloned())
        }

        fn read_object_bounded(
            &self,
            space: SpaceId,
            id: &ObjectId,
            _max_bytes: usize,
        ) -> Result<Option<Vec<u8>>, StorageError> {
            self.read_object(space, id)
        }
    }

    fn name(text: &str) -> CellName {
        CellName::new(text).unwrap()
    }

    fn record(key: &[u8], kind: &str) -> WriteOp {
        WriteOp::CreateRecord {
            namespace: NamespaceId::from_ordinal(1).unwrap(),
            key: RecordKey::new(key).unwrap(),
            cells: vec![
                (name("kind"), Cell::attribute(Value::str(kind))),
                (name("n"), Cell::attribute(Value::I64(7))),
            ],
        }
    }

    /// Hosts share snapshots across threads and move branches and sealed
    /// candidates between them.
    #[test]
    fn engine_handles_are_send_and_sync() {
        fn send_sync<T: Send + Sync>() {}
        send_sync::<Engine>();
        send_sync::<Snapshot>();
        send_sync::<Branch>();
        send_sync::<SealedCandidate>();
    }

    #[test]
    fn a_failed_operation_leaves_the_branch_state_as_it_was() {
        let owner = OwnerId::new([1; 20]);
        let mut memory = Engine::new(PriceSchedule::FIXTURE);
        let mut branch = memory.begin(memory.head(), None).unwrap();
        let create = WriteOp::CreateNamespace {
            name: NamespaceName::new("alpha").unwrap(),
        };
        branch.execute(owner, create, Budget::unlimited()).unwrap();
        for index in 0u8..8 {
            let kind = ["red", "rose"][usize::from(index % 2)];
            let op = record(&[b'r', index], kind);
            branch.execute(owner, op, Budget::unlimited()).unwrap();
        }
        for key in [[1; 32], [2; 32]] {
            branch.try_host_put(key, b"due").unwrap().unwrap();
        }
        memory.publish(&branch.seal()).unwrap();
        let head = memory.head_snapshot();
        let reader = Arc::new(Reader {
            objects: head
                .staged_objects()
                .into_iter()
                .map(|object| ((object.space, object.id), object.bytes))
                .collect(),
            allowance: AtomicU64::new(u64::MAX),
            reads: AtomicU64::new(0),
        });
        let engine =
            Engine::from_state(&head.state_header(), Limits::DEFAULT, reader.clone()).unwrap();

        // The operation under test, after one earlier applied write (to the
        // raw map, so the typed maps stay stored) so the working state
        // differs from the base.
        let failing = record(b"new", "ruby");
        let prepare = |branch: &mut Branch| {
            reader.allowance.store(u64::MAX, Ordering::SeqCst);
            let op = WriteOp::Put {
                namespace: NamespaceId::from_ordinal(1).unwrap(),
                key: b"raw".to_vec(),
                value: b"value".to_vec(),
            };
            branch.execute(owner, op, Budget::unlimited()).unwrap();
        };
        let mut branch = engine.begin(engine.head(), None).unwrap();
        prepare(&mut branch);
        let before = reader.reads.load(Ordering::SeqCst);
        branch
            .execute(owner, failing.clone(), Budget::unlimited())
            .unwrap();
        let needed = reader.reads.load(Ordering::SeqCst) - before;
        assert!(needed > 4, "the create loads the stored typed maps");

        for allowance in 0..needed {
            let mut branch = engine.begin(engine.head(), None).unwrap();
            prepare(&mut branch);
            let state = branch.working.header();
            let mutations = branch.mutations;
            reader.allowance.store(allowance, Ordering::SeqCst);
            let result = branch.try_execute(owner, failing.clone(), Budget::unlimited());
            assert!(result.is_err(), "failing after {allowance} of {needed}");
            assert_eq!(branch.working.header(), state, "after {allowance}");
            assert_eq!(branch.mutations, mutations);
            assert_eq!(branch.receipts.len(), 1, "only the earlier write");
            assert!(branch.is_poisoned());
        }

        // A failing host-state write (the host map is stored) changes
        // nothing either.
        let mut branch = engine.begin(engine.head(), None).unwrap();
        prepare(&mut branch);
        let state = branch.working.header();
        reader.allowance.store(0, Ordering::SeqCst);
        assert!(branch.try_host_put([3; 32], b"later").is_err());
        assert_eq!(branch.working.header(), state);
        assert!(branch.is_poisoned());
    }
}
