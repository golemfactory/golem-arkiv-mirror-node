//! State-history retention of the durable engine: the strict floor, its
//! persistence and monotonicity across reopen, pins, and the read-only
//! reachability report. Retention only refuses reads below the floor; no
//! object is deleted.

use std::sync::Arc;

use arkiv_db::{
    EngineError, StorageErrorKind,
    durable::{DurableEngine, DurableError, Options, Retention, reachability::reachability},
};
use arkiv_db_memory::MemoryVersionedStore;
use arkiv_db_storage::versioned::{SpaceId, Table, VersionedStore};

use crate::support::{
    durable::{commit, genesis, initialize, put_candidate, snapshot_id, value},
    vocab::{OWNER, apply, ns, put},
};

fn options(retention: Retention) -> Options {
    Options {
        retention,
        ..Options::default()
    }
}

/// Publishes `count` blocks, each storing its own sequence number.
fn advance(engine: &mut DurableEngine, count: u64) {
    for _ in 0..count {
        let next = engine.head().sequence() + 1;
        let candidate = put_candidate(engine, &next.to_be_bytes());
        commit(engine, candidate);
    }
}

fn pruned(engine: &DurableEngine, requested: u64) -> (u64, u64) {
    match engine.snapshot(snapshot_id(requested)) {
        Err(DurableError::Engine(EngineError::HistoryPruned { requested, floor })) => {
            (requested.sequence(), floor.sequence())
        }
        other => panic!("expected HistoryPruned for {requested}, got {other:?}"),
    }
}

fn floor_record(store: &MemoryVersionedStore, sequence: u64) -> Option<u64> {
    store
        .read_record(
            Table::Meta,
            format!("engine/state-floor/{sequence:016x}").as_bytes(),
        )
        .unwrap()
        .map(|b| u64::from_be_bytes(b.try_into().unwrap()))
}

#[test]
fn archive_is_the_default_and_serves_every_snapshot() {
    assert_eq!(Options::default().retention, Retention::Archive);
    let store = Arc::new(MemoryVersionedStore::new());
    let mut engine = initialize(store.clone(), options(Retention::Archive));
    advance(&mut engine, 5);
    assert_eq!(engine.state_floor(), snapshot_id(0));
    for sequence in 1..=5 {
        assert_eq!(
            value(&engine.snapshot(snapshot_id(sequence)).unwrap()),
            sequence.to_be_bytes()
        );
    }
    engine.snapshot(snapshot_id(0)).unwrap();
    // Archive commits record no floor.
    assert!((0..=5).all(|s| floor_record(&store, s).is_none()));
    let status = engine.retention_status();
    assert_eq!(
        (status.retention, status.floor, status.durable_floor),
        (Retention::Archive, 0, 0)
    );
}

#[test]
fn window_refuses_below_the_floor_before_any_read() {
    let store = Arc::new(MemoryVersionedStore::new());
    let mut engine = initialize(store.clone(), options(Retention::Window(3)));
    advance(&mut engine, 2);
    // head 2 - 3 saturates: everything is still served.
    assert_eq!(engine.state_floor(), snapshot_id(0));
    engine.snapshot(snapshot_id(0)).unwrap();
    advance(&mut engine, 8);
    assert_eq!(engine.head(), snapshot_id(10));
    assert_eq!(engine.state_floor(), snapshot_id(7));
    // At the floor and above: served. Below: refused, although every object
    // is still stored, and without reading a manifest or an object.
    for sequence in 7..=10 {
        assert_eq!(
            value(&engine.snapshot(snapshot_id(sequence)).unwrap()),
            sequence.to_be_bytes()
        );
    }
    let before = store.stats();
    for sequence in 0..7 {
        assert_eq!(pruned(&engine, sequence), (sequence, 7));
    }
    let after = store.stats().since(&before);
    assert_eq!(after.record_reads, 0);
    assert_eq!(after.object_reads(), 0);
    assert_eq!(
        engine
            .check_retained(snapshot_id(6))
            .unwrap_err()
            .to_string(),
        "history pruned: requested 6, floor 7"
    );
    // Above the head stays UnknownSnapshot.
    assert_eq!(
        engine.snapshot(snapshot_id(11)).unwrap_err(),
        DurableError::Engine(EngineError::UnknownSnapshot(snapshot_id(11)))
    );
    // The floor is recorded with every commit whose floor is above zero.
    assert_eq!(floor_record(&store, 10), Some(7));
    assert_eq!(floor_record(&store, 4), Some(1));
    assert_eq!(floor_record(&store, 3), None);
    assert_eq!(engine.stats().retention.floor, 7);
}

#[test]
fn window_zero_serves_only_the_head() {
    let store = Arc::new(MemoryVersionedStore::new());
    let mut engine = initialize(store, options(Retention::Window(0)));
    engine.snapshot(snapshot_id(0)).unwrap();
    advance(&mut engine, 3);
    assert_eq!(engine.state_floor(), snapshot_id(3));
    assert_eq!(
        value(&engine.snapshot(snapshot_id(3)).unwrap()),
        3u64.to_be_bytes()
    );
    assert_eq!(pruned(&engine, 2), (2, 3));
    assert_eq!(pruned(&engine, 0), (0, 3));
}

#[test]
fn floor_persists_and_never_moves_backwards_across_reopen() {
    let store = Arc::new(MemoryVersionedStore::new());
    let mut engine = initialize(store.clone(), options(Retention::Window(4)));
    advance(&mut engine, 10);
    assert_eq!(engine.state_floor(), snapshot_id(6));
    drop(engine);

    // A larger window cannot bring declared-pruned history back.
    let mut engine =
        DurableEngine::open(store.clone(), genesis(), options(Retention::Window(100))).unwrap();
    assert_eq!(engine.state_floor(), snapshot_id(6));
    assert_eq!(pruned(&engine, 5), (5, 6));
    advance(&mut engine, 2);
    assert_eq!(engine.state_floor(), snapshot_id(6));
    assert_eq!(floor_record(&store, 12), Some(6));
    drop(engine);

    // Archive after pruning is refused.
    let error =
        DurableEngine::open(store.clone(), genesis(), options(Retention::Archive)).unwrap_err();
    assert!(
        matches!(&error, DurableError::Storage(e) if e.kind == StorageErrorKind::Incompatible),
        "{error:?}"
    );

    // A smaller window raises the floor at once for reads; it becomes
    // durable with the next commit.
    let mut engine =
        DurableEngine::open(store.clone(), genesis(), options(Retention::Window(2))).unwrap();
    let status = engine.retention_status();
    assert_eq!((status.floor, status.durable_floor), (10, 6));
    assert_eq!(pruned(&engine, 9), (9, 10));
    advance(&mut engine, 1);
    let status = engine.retention_status();
    assert_eq!((status.floor, status.durable_floor), (11, 11));
    assert_eq!(floor_record(&store, 13), Some(11));
    drop(engine);
    let engine = DurableEngine::open(store, genesis(), options(Retention::Window(50))).unwrap();
    assert_eq!(engine.state_floor(), snapshot_id(11));
}

#[test]
fn archive_reopens_a_window_database_that_never_pruned() {
    let store = Arc::new(MemoryVersionedStore::new());
    let mut engine = initialize(store.clone(), options(Retention::Window(10)));
    advance(&mut engine, 5);
    drop(engine);
    let engine = DurableEngine::open(store, genesis(), options(Retention::Archive)).unwrap();
    engine.snapshot(snapshot_id(0)).unwrap();
}

#[test]
fn pins_track_open_handles_and_old_handles_keep_reading() {
    let store = Arc::new(MemoryVersionedStore::new());
    let mut engine = initialize(store, options(Retention::Window(1)));
    advance(&mut engine, 3);
    // Only the head is held by the engine.
    let status = engine.retention_status();
    assert_eq!((status.lowest_pinned, status.pinned_handles), (Some(3), 1));
    let old = engine.snapshot(snapshot_id(2)).unwrap();
    let clone = old.clone();
    let status = engine.retention_status();
    assert_eq!((status.lowest_pinned, status.pinned_handles), (Some(2), 2));
    advance(&mut engine, 2);
    // The floor passed the pinned handle; new opens are refused, the open
    // handle keeps reading until it is dropped (nothing is deleted).
    assert_eq!(engine.state_floor(), snapshot_id(4));
    assert_eq!(pruned(&engine, 2), (2, 4));
    assert_eq!(value(&clone), 2u64.to_be_bytes());
    assert_eq!(engine.retention_status().lowest_pinned, Some(2));
    drop(old);
    assert_eq!(engine.retention_status().lowest_pinned, Some(2));
    drop(clone);
    assert_eq!(engine.retention_status().lowest_pinned, Some(5));
    // A branch pins its base until it is dropped.
    let branch = engine.begin(engine.head(), None).unwrap();
    let head = engine.snapshot(snapshot_id(5)).unwrap();
    publish_with(&mut engine, branch);
    assert_eq!(engine.retention_status().lowest_pinned, Some(5));
    drop(head);
    assert_eq!(engine.retention_status().lowest_pinned, Some(6));
}

fn publish_with(engine: &mut DurableEngine, mut branch: arkiv_db::Branch) {
    apply(&mut branch, OWNER, put(ns(1), b"key", b"branch"));
    engine.prepare(branch.try_seal().unwrap()).unwrap();
    engine.commit().unwrap();
}

#[test]
fn reachability_counts_head_and_recent_unions() {
    let store = Arc::new(MemoryVersionedStore::new());
    let mut engine = initialize(store.clone(), options(Retention::Archive));
    advance(&mut engine, 20);
    let report = reachability(store.as_ref(), &[1, 5, 1_000]).unwrap();
    assert_eq!(report.head, 20);
    assert_eq!(report.levels.len(), 3);
    let [head, five, all] = [0, 1, 2].map(|i| report.levels[i].clone());
    assert_eq!((head.recent, head.oldest), (1, 20));
    assert_eq!((five.oldest, all.oldest), (16, 0));
    // Unions only grow, and the archive union is every stored object.
    for space in [SpaceId::Catalog, SpaceId::Namespace(1)] {
        let at = |l: &arkiv_db::durable::reachability::ReachLevel| l.per_space[&space].objects;
        assert!(at(&head) < at(&five) && at(&five) < at(&all), "{space:?}");
    }
    let stats = store.stats();
    let stored: u64 = report
        .spaces()
        .iter()
        .filter_map(|s| stats.spaces.get(s))
        .map(|s| s.objects_written)
        .sum();
    assert_eq!(all.total().objects, stored);
}

#[test]
fn reachability_is_clamped_to_the_floor_and_pins_its_oldest_level() {
    let store = Arc::new(MemoryVersionedStore::new());
    let mut engine = initialize(store.clone(), options(Retention::Window(4)));
    advance(&mut engine, 12);
    assert_eq!(engine.state_floor(), snapshot_id(8));
    let live = engine.reachability(&[1, 3, 100]).unwrap();
    assert_eq!(live.floor, 8);
    let oldest: Vec<_> = live.levels.iter().map(|l| l.oldest).collect();
    assert_eq!(oldest, [12, 10, 8]);
    // The offline report uses the durable floor recorded with the head.
    let offline = reachability(store.as_ref(), &[1, 3, 100]).unwrap();
    assert_eq!(offline, live);
    // The walk released its pin.
    assert_eq!(engine.retention_status().lowest_pinned, Some(12));
}
