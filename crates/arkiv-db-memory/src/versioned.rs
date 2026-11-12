//! The volatile reference implementation of the versioned storage contract.

use std::{
    collections::{BTreeMap, HashMap},
    sync::{Arc, PoisonError, RwLock, RwLockReadGuard, RwLockWriteGuard},
};

use arkiv_db_storage::versioned::{
    CommitBatch, CommitOutcome, DatabaseId, Head, Health, ObjectId, ObjectReader, Relabel, SpaceId,
    StatsRecorder, StorageError, StoreStats, Table, VersionedStore, Written, copy_bounded,
    head_mismatch, object_conflict, record_conflict, relabel_mismatch, validate_objects,
};

/// A [`VersionedStore`] held entirely in process memory.
///
/// It implements the whole contract — put-if-absent objects per space,
/// insert-only records, the head compare-and-set and atomic batches — under
/// one lock, and serves as the reference the durable stores are compared
/// with. It has no I/O failures: health is always `Available`, and `recover`
/// only returns the head. Bounded reads still reject oversized values.
/// Everything is lost when the store is dropped; it does
/// not verify content addresses, because nothing can damage its bytes.
#[derive(Debug)]
pub struct MemoryVersionedStore {
    id: DatabaseId,
    state: RwLock<MemoryState>,
    stats: StatsRecorder,
}

#[derive(Debug, Default)]
struct MemoryState {
    head: Option<Head>,
    objects: BTreeMap<SpaceId, HashMap<ObjectId, Arc<[u8]>>>,
    records: BTreeMap<(Table, Vec<u8>), Vec<u8>>,
}

impl MemoryVersionedStore {
    /// Creates an empty store with a random [`DatabaseId`].
    pub fn new() -> Self {
        Self::with_database_id(DatabaseId::random())
    }

    /// Creates an empty store with the given identity.
    pub fn with_database_id(id: DatabaseId) -> Self {
        Self {
            id,
            state: RwLock::new(MemoryState::default()),
            stats: StatsRecorder::new(),
        }
    }

    fn read(&self) -> RwLockReadGuard<'_, MemoryState> {
        // Every mutation happens after all checks, in infallible map
        // operations, so a poisoned lock still guards consistent state.
        self.state.read().unwrap_or_else(PoisonError::into_inner)
    }

    fn write(&self) -> RwLockWriteGuard<'_, MemoryState> {
        self.state.write().unwrap_or_else(PoisonError::into_inner)
    }
}

impl Default for MemoryVersionedStore {
    fn default() -> Self {
        Self::new()
    }
}

impl ObjectReader for MemoryVersionedStore {
    fn read_object_bounded(
        &self,
        space: SpaceId,
        id: &ObjectId,
        max_bytes: usize,
    ) -> Result<Option<Vec<u8>>, StorageError> {
        let found = self
            .read()
            .objects
            .get(&space)
            .and_then(|objects| objects.get(id))
            .map(|bytes| copy_bounded(bytes, max_bytes))
            .transpose()?;
        self.stats.object_read(space, found.as_ref().map(Vec::len));
        Ok(found)
    }
}

impl VersionedStore for MemoryVersionedStore {
    fn database_id(&self) -> DatabaseId {
        self.id
    }

    fn head(&self) -> Result<Option<Head>, StorageError> {
        let head = self.read().head;
        self.stats.head_read();
        Ok(head)
    }

    fn read_record_bounded(
        &self,
        table: Table,
        key: &[u8],
        max_bytes: usize,
    ) -> Result<Option<Vec<u8>>, StorageError> {
        let found = self
            .read()
            .records
            .get(&(table, key.to_vec()))
            .map(|bytes| copy_bounded(bytes, max_bytes))
            .transpose()?;
        self.stats.record_read(found.as_ref().map(Vec::len));
        Ok(found)
    }

    fn commit(&self, batch: &CommitBatch) -> CommitOutcome {
        self.stats
            .write(self.apply(batch).map_err(CommitOutcome::NotCommitted))
    }

    fn stage_objects(&self, objects: &[(SpaceId, ObjectId, Vec<u8>)]) -> CommitOutcome {
        // A refused staging is not counted.
        match self.apply_objects(objects) {
            Ok(written) => self.stats.write(Ok(written)),
            Err(error) => CommitOutcome::NotCommitted(error),
        }
    }

    fn relabel(&self, relabel: &Relabel) -> CommitOutcome {
        self.stats.write(
            self.apply_relabel(relabel)
                .map_err(CommitOutcome::NotCommitted),
        )
    }

    fn health(&self) -> Health {
        Health::Available
    }

    fn recover(&self) -> Result<Option<Head>, StorageError> {
        let head = self.read().head;
        self.stats.recovered();
        Ok(head)
    }

    fn stats(&self) -> StoreStats {
        self.stats.snapshot(None)
    }
}

impl MemoryVersionedStore {
    /// Checks the whole batch against the current state, then applies it.
    fn apply(&self, batch: &CommitBatch) -> Result<Written, StorageError> {
        batch.validate()?;
        self.write().put(
            batch.expected_head,
            &batch.objects,
            &batch.records,
            Some(batch.new_head),
        )
    }

    /// Puts objects into a store that has no head, leaving it headless.
    fn apply_objects(
        &self,
        objects: &[(SpaceId, ObjectId, Vec<u8>)],
    ) -> Result<Written, StorageError> {
        validate_objects(objects)?;
        self.write().put(None, objects, &[], None)
    }

    /// Checks the retired records and the head, then replaces them with the
    /// batch under one lock (see [`Relabel`]).
    fn apply_relabel(&self, relabel: &Relabel) -> Result<Written, StorageError> {
        relabel.validate()?;
        let mut state = self.write();
        if state.head != Some(relabel.expected_head) {
            return Err(head_mismatch(Some(relabel.expected_head), state.head));
        }
        if state.records.len() != relabel.retired.len() {
            return Err(relabel_mismatch(format!(
                "{} records, {} retired",
                state.records.len(),
                relabel.retired.len()
            )));
        }
        for (table, key, value) in &relabel.retired {
            if state.records.get(&(*table, key.clone())) != Some(value) {
                return Err(relabel_mismatch(format!("{} record differs", table.name())));
            }
        }
        // The batch is checked against the emptied tables before anything
        // changes, so a refusal leaves the store as it was.
        let mut relabelled = MemoryState {
            head: None,
            objects: std::mem::take(&mut state.objects),
            records: BTreeMap::new(),
        };
        let batch = &relabel.batch;
        let result = relabelled.put(None, &batch.objects, &batch.records, Some(batch.new_head));
        if result.is_err() {
            state.objects = relabelled.objects;
        } else {
            *state = relabelled;
        }
        result
    }
}

impl MemoryState {
    /// Checks the entries against `expected_head` and the stored values,
    /// then stores the new ones and sets `new_head` if there is one.
    fn put(
        &mut self,
        expected_head: Option<Head>,
        objects: &[(SpaceId, ObjectId, Vec<u8>)],
        records: &[(Table, Vec<u8>, Vec<u8>)],
        new_head: Option<Head>,
    ) -> Result<Written, StorageError> {
        if self.head != expected_head {
            return Err(head_mismatch(expected_head, self.head));
        }
        for (space, id, bytes) in objects {
            if let Some(existing) = self.objects.get(space).and_then(|objects| objects.get(id))
                && **existing != **bytes
            {
                return Err(object_conflict(*space, id));
            }
        }
        for (table, key, value) in records {
            if let Some(existing) = self.records.get(&(*table, key.clone()))
                && existing != value
            {
                return Err(record_conflict(*table, key));
            }
        }
        // Nothing below can fail: the entries apply completely.
        let mut written = Written::default();
        for (space, id, bytes) in objects {
            let stored = self.objects.entry(*space).or_default();
            if let std::collections::hash_map::Entry::Vacant(slot) = stored.entry(*id) {
                written.objects.push((*space, bytes.len()));
                slot.insert(bytes.as_slice().into());
            }
        }
        for (table, key, value) in records {
            if let std::collections::btree_map::Entry::Vacant(slot) =
                self.records.entry((*table, key.clone()))
            {
                written.records += 1;
                slot.insert(value.clone());
            }
        }
        if new_head.is_some() {
            self.head = new_head;
        }
        Ok(written)
    }
}
