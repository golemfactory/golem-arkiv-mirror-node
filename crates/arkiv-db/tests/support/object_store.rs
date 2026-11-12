//! Committed state held in an object store, for tests that reopen an engine
//! over objects they can count, fail, hide and replace.

use std::sync::Arc;

use alloy_primitives::keccak256;
use arkiv_db::{Engine, Limits, StagedObject, StateHeader};
use arkiv_db_memory::MemoryVersionedStore;
use arkiv_db_storage::{
    testing::FaultyStore,
    versioned::{CommitOutcome, VersionedStore},
};

/// Counts object reads (`stats().object_reads()`), fails them
/// (`fail_reads_after`) and hides or replaces single objects
/// (`override_object`).
pub type ObjectStore = FaultyStore<MemoryVersionedStore>;

/// Adds `objects` to `store`.
pub fn stage(store: &ObjectStore, objects: Vec<StagedObject>) {
    let objects: Vec<_> = objects
        .into_iter()
        .map(|object| {
            assert_eq!(
                keccak256(&object.bytes).0,
                object.id,
                "objects are content addressed"
            );
            (object.space, object.id, object.bytes)
        })
        .collect();
    assert_eq!(store.stage_objects(&objects), CommitOutcome::Committed);
}

/// A store holding `objects`, and an engine reading the state `header`
/// names from it.
pub fn reopen(header: &StateHeader, objects: Vec<StagedObject>) -> (Arc<ObjectStore>, Engine) {
    let store = Arc::new(FaultyStore::new(MemoryVersionedStore::new()));
    stage(&store, objects);
    let engine = Engine::from_state(header, Limits::DEFAULT, store.clone())
        .expect("the header names a readable state");
    (store, engine)
}
