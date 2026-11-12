use crate::common::{BenchDir, keccak};
use arkiv_db::{
    PriceSchedule,
    durable::{DurableEngine, EngineGenesis, Initialization, Options},
};
use arkiv_db_redb::{RedbConfig, RedbStore};
use arkiv_db_storage::versioned::{CommitOutcome, ObjectReader, SpaceId, VersionedStore};
use std::sync::Arc;
fn committed(i: Initialization) -> DurableEngine {
    match i {
        Initialization::Committed(e) => e,
        other => panic!("{other:?}"),
    }
}
#[test]
fn headless_objects_survive_reopen_and_cannot_mutate_published_store() {
    let dir = BenchDir::new("headless-import");
    let path = dir.file("target.redb");
    let bytes = b"immutable staging object".to_vec();
    let id = keccak(&bytes);
    let batch = vec![(SpaceId::Host, id, bytes.clone())];
    let store = RedbStore::create(&path, RedbConfig::default()).unwrap();
    assert_eq!(store.stage_objects(&batch), CommitOutcome::Committed);
    assert_eq!(store.head().unwrap(), None);
    drop(store);
    let store = Arc::new(RedbStore::open(&path, RedbConfig::default()).unwrap());
    assert_eq!(store.read_object(SpaceId::Host, &id).unwrap(), Some(bytes));
    assert_eq!(store.stage_objects(&batch), CommitOutcome::Committed);
    let genesis = EngineGenesis::new([1; 32], PriceSchedule::FIXTURE);
    assert!(DurableEngine::open(store.clone(), genesis.clone(), Options::default()).is_err());
    let _engine =
        committed(DurableEngine::initialize(store.clone(), genesis, Options::default()).unwrap());
    assert!(matches!(
        store.stage_objects(&batch),
        CommitOutcome::NotCommitted(_)
    ));
}
