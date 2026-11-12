//! Historical objects stay directly readable: after reopening with a tiny
//! cache, and while new blocks commit.
//!
//! Every block writes new objects into three namespaces, including a new
//! version of the same logical entries (new bytes, so a new content address)
//! each block, the way a persistent tree writes a new node for every changed
//! path while old nodes stay reachable from old roots.
//!
//! - After 60 blocks and a reopen with a zero-byte and with a 64 KiB redb
//!   cache, every object and record of every block reads back identical.
//! - While a writer commits 80 blocks, four readers repeatedly read the head
//!   and verify every object and record of a block at or below it; a
//!   committed block is never missing or different.
//! - The store never created a redb savepoint: history is ordinary immutable
//!   entries of the current tables, not restored snapshots.

use crate::common::{BenchDir, Chain, assert_applied, commit, history::concurrent_history};
use arkiv_db_redb::{RedbConfig, RedbStore};
use arkiv_db_storage::versioned::{SpaceId, VersionedStore};

fn history_objects(height: u64) -> Vec<(SpaceId, u64, usize)> {
    vec![
        (SpaceId::Namespace(1), 12, 180),
        (SpaceId::Namespace(2), 3, 1_500),
        (SpaceId::Namespace(3 + height % 5), 5, 90),
        (SpaceId::Catalog, 2, 110),
    ]
}

#[test]
fn every_historical_object_is_readable_after_reopen_with_a_small_cache() {
    let dir = BenchDir::new("history");
    let path = dir.file("store.redb");
    let store = RedbStore::create(&path, RedbConfig::default()).unwrap();
    let mut chain = Chain::default();
    let mut blocks = Vec::new();
    for height in 0..60 {
        let batch = chain.next(&history_objects(height));
        commit(&store, &batch);
        blocks.push(batch);
    }
    drop(store);

    for cache in [0, 64 << 10] {
        let store = RedbStore::open(&path, RedbConfig::with_cache_size(cache)).unwrap();
        assert_eq!(store.head().unwrap(), chain.head());
        for batch in &blocks {
            assert_applied(&store, batch);
        }
    }

    let db = redb::Database::open(&path).unwrap();
    let txn = db.begin_write().unwrap();
    assert_eq!(txn.list_persistent_savepoints().unwrap().count(), 0);
    txn.abort().unwrap();
}

#[test]
fn historical_reads_run_concurrently_with_commits() {
    let dir = BenchDir::new("history-concurrent");
    let store = RedbStore::create(
        dir.file("store.redb"),
        RedbConfig::with_cache_size(128 << 10),
    )
    .unwrap();
    let history = concurrent_history(&store, 80, history_objects);
    assert!(history.verified_objects > 0);
    assert_eq!(history.blocks.len(), 80);
    for batch in &history.blocks {
        assert_applied(&store, batch);
    }
}
