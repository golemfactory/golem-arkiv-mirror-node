//! Memory/durable differential histories, format vectors and lazy ownership.
use arkiv_db::{
    Budget, OwnerId, PriceSchedule, Projection, Query, SnapshotId, Value, decode_exact,
    durable::{DurableEngine, EngineGenesis, EngineManifest, Options},
    encode,
};
use arkiv_db_memory::MemoryVersionedStore;
use arkiv_db_storage::versioned::{SpaceId, VersionedStore};
use std::sync::Arc;

use crate::support::{
    durable::{commit, domain_hash, genesis, initialize},
    vocab::{OWNER, add_namespace, apply, attr, create_record, name, ns, transfer},
};

#[test]
fn hand_built_profile_and_manifest_vectors() {
    let g = EngineGenesis::new([9; 32], PriceSchedule::uniform(1, 2));
    let mut expected = b"ARKENG01".to_vec();
    for version in [2u32, 1, 2, 3, 2, 2] {
        expected.extend(version.to_be_bytes());
    }
    expected.extend([9; 32]);
    for limit in [1u64 << 32, 64, 32, 4096, 4096, 128, 200, 8, 8] {
        expected.extend(limit.to_be_bytes());
    }
    expected.extend(1u32.to_be_bytes());
    expected.extend(31u32.to_be_bytes());
    for _ in 0..31 {
        expected.extend(2u64.to_be_bytes());
    }
    expected.extend((1024u64 * 1024).to_be_bytes());
    expected.push(0); // no initial state
    // This vector deliberately pins the initial schedule rather than taking
    // it from the encoding under test.
    assert_eq!(encode(&g), expected);
    assert_eq!(decode_exact::<EngineGenesis>(&expected).unwrap(), g);
    assert_eq!(g.id(), domain_hash("arkiv/engine-genesis/v1", &expected));
    let header = g.initial_header();
    let manifest = EngineManifest {
        genesis: g.id(),
        sequence: 0,
        parent: None,
        state_root: header.root(),
        state: header.clone(),
    };
    let mut bytes = 1u32.to_be_bytes().to_vec();
    bytes.extend(g.id());
    bytes.extend(0u64.to_be_bytes());
    bytes.push(0);
    let empty = arkiv_db_trie::EMPTY_ROOT_HASH;
    bytes.extend(2u32.to_be_bytes()); // state format 2
    bytes.extend(empty);
    bytes.extend(0u64.to_be_bytes());
    bytes.extend(1u64.to_be_bytes());
    bytes.extend(&expected[136..392]); // the pricing
    bytes.extend(empty);
    bytes.extend(0u64.to_be_bytes());
    bytes.extend(header.root());
    assert_eq!(encode(&manifest), bytes);
    assert_eq!(decode_exact::<EngineManifest>(&bytes).unwrap(), manifest);
    assert_eq!(
        manifest.id(),
        domain_hash("arkiv/engine-manifest/v1", &bytes)
    );
    let successor = EngineManifest {
        sequence: 1,
        parent: Some((0, manifest.id())),
        ..manifest.clone()
    };
    let mut successor_bytes = bytes[..36].to_vec();
    successor_bytes.extend(1u64.to_be_bytes());
    successor_bytes.push(1);
    successor_bytes.extend(0u64.to_be_bytes());
    successor_bytes.extend(manifest.id());
    successor_bytes.extend(&bytes[45..]);
    assert_eq!(encode(&successor), successor_bytes);
    assert_eq!(
        decode_exact::<EngineManifest>(&successor_bytes).unwrap(),
        successor
    );
    assert_eq!(
        successor.id(),
        domain_hash("arkiv/engine-manifest/v1", &successor_bytes)
    );
    assert_ne!(successor.id(), manifest.id());
    assert_eq!(successor.state_root, manifest.state_root);
    for n in 0..bytes.len() {
        assert!(decode_exact::<EngineManifest>(&bytes[..n]).is_err());
    }
    let mut trailing = bytes.clone();
    trailing.push(0);
    assert!(decode_exact::<EngineManifest>(&trailing).is_err());
    let mut wrong_version = bytes;
    wrong_version[3] = 2;
    assert!(decode_exact::<EngineManifest>(&wrong_version).is_err());
}

#[test]
fn seeded_histories_reopen_with_every_cache_size_and_preserve_proofs_receipts_roots() {
    for seed in [1u64, 27] {
        let store = Arc::new(MemoryVersionedStore::new());
        crate::durable_history::run_seeded_history(store.clone(), move || store.clone(), seed);
    }
}

#[test]
fn owner_transfer_only_stages_catalog_and_recovery_preserves_same_open_cursor_identity() {
    let store = Arc::new(MemoryVersionedStore::new());
    let mut engine = initialize(store.clone(), Options::default());
    let mut b = engine.begin(engine.head(), None).unwrap();
    add_namespace(&mut b, OWNER, "one");
    for i in 0..20u64 {
        let cells = vec![(name("kind"), attr(Value::str("book")))];
        apply(&mut b, OWNER, create_record(ns(1), i.to_be_bytes(), cells));
    }
    commit(&mut engine, b.seal());
    let old = engine.head_snapshot();
    let q = Query::all(vec![]);
    let p = Projection::ALL;
    let cursor = old
        .query(ns(1), &q, &p, 1, None, Budget::unlimited())
        .unwrap()
        .value
        .next
        .unwrap();
    let same = engine.snapshot(old.id()).unwrap();
    assert!(
        same.query(ns(1), &q, &p, 1, Some(&cursor), Budget::unlimited())
            .is_ok()
    );
    engine.recover().unwrap();
    assert!(
        same.query(ns(1), &q, &p, 1, Some(&cursor), Budget::unlimited())
            .is_ok()
    );
    let mut b = engine.begin(engine.head(), None).unwrap();
    apply(&mut b, OWNER, transfer(ns(1), 0, OwnerId::new([8; 20])));
    let before = store.stats();
    commit(&mut engine, b.seal());
    let delta = store.stats().since(&before);
    assert_eq!(delta.space(SpaceId::Namespace(1)).objects_written, 0);
    assert_eq!(old.state_root(), same.state_root());
    drop(engine);
    let reopened = DurableEngine::open(store, genesis(), Options::default()).unwrap();
    assert!(
        reopened
            .snapshot(old.id())
            .unwrap()
            .query(ns(1), &q, &p, 1, Some(&cursor), Budget::unlimited())
            .is_err()
    );
}

#[test]
fn ten_thousand_publications_retain_no_resident_history_or_dirty_committed_nodes() {
    let store = Arc::new(MemoryVersionedStore::new());
    let mut engine = initialize(
        store,
        Options {
            cache_bytes: 64 * 1024,
            ..Options::default()
        },
    );
    for sequence in 1..=10_000u64 {
        let mut b = engine.begin(engine.head(), None).unwrap();
        b.try_host_put([1; 32], &sequence.to_be_bytes())
            .unwrap()
            .unwrap();
        commit(&mut engine, b.seal());
        assert_eq!(engine.stats().resident_manifests, 1);
        assert_eq!(engine.stats().pending_payload_bytes, 0);
        assert_eq!(engine.head().sequence(), sequence);
        assert!(engine.head_snapshot().staged_objects().is_empty());
        assert!(engine.stats().cache.retained_bytes <= 64 * 1024);
        assert_eq!(engine.stats().last_preparation.unique_objects, 1);
    }
    assert_eq!(engine.snapshot_count().unwrap(), 10_001);
    let handles: Vec<_> = (1..=1000)
        .map(|n| engine.snapshot(SnapshotId::from_sequence(n)).unwrap())
        .collect();
    for handle in &handles {
        assert_eq!(
            handle.try_host_get(&[1; 32]).unwrap().unwrap(),
            handle.id().sequence().to_be_bytes()
        );
        assert!(handle.staged_objects().is_empty());
    }
    drop(handles);
    assert_eq!(engine.stats().resident_manifests, 1);
}
