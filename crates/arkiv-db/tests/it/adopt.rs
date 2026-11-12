//! Adopting a bulk-staged store in place as a simulator database: the result
//! equals the copy installation, only a single exact bulk publication is
//! eligible, and every refusal or failure before the relabelling commit
//! leaves the staged store as it was.
use arkiv_db::{
    B256, Budget, Limits, Literal, PriceSchedule, Projection, Query, Term, Value,
    durable::{
        BulkBuild, BulkGenesisBuilder, DurableEngine, EngineGenesis, Options, Publication,
        simulator::{SimulatorPacket, StorageProfile},
    },
};
use arkiv_db_memory::MemoryVersionedStore;
use arkiv_db_storage::{
    testing::{CommitFault, FaultyStore},
    versioned::{CommitBatch, CommitOutcome, SpaceId, StorageErrorKind, Table, VersionedStore},
};
use std::sync::Arc;

use crate::support::{
    durable::{commit, committed, entities, node},
    vocab::{attr, field, key, name},
};

/// The objects of a bulk-built state staged on `store`, still unpublished.
fn build(store: Arc<dyn VersionedStore>, tag: &str) -> BulkBuild {
    let dir = std::env::temp_dir().join(format!("arkiv-adopt-it-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let mut builder = BulkGenesisBuilder::new(&dir, entities(), Limits::DEFAULT, 4096).unwrap();
    for n in 0..200u64 {
        builder
            .add_record(
                key(n.to_be_bytes()),
                vec![
                    (name("kind"), attr(Value::str("fixture"))),
                    (name("mod"), attr(Value::U64(n % 5))),
                    (name("n"), attr(Value::U64(n))),
                    (name("payload"), field(Value::bytes(&[n as u8; 64]))),
                ],
            )
            .unwrap();
    }
    for n in 0..20u8 {
        builder.add_host([n; 32], vec![n; 24]).unwrap();
    }
    let build = builder
        .finish(store, PriceSchedule::FIXTURE, Options::default())
        .unwrap();
    std::fs::remove_dir_all(&dir).unwrap();
    build
}

/// A bulk-built staged store on `store`, as the streaming prefill builds it.
fn staged(store: Arc<dyn VersionedStore>, tag: &str) -> (DurableEngine, EngineGenesis) {
    let build = build(store, tag);
    let mut genesis = EngineGenesis::new([5; 32], PriceSchedule::FIXTURE);
    genesis.initial_state = Some(build.state_header().clone());
    let engine = committed(
        DurableEngine::initialize_bulk(build, genesis.clone(), Options::default()).unwrap(),
    );
    (engine, genesis)
}

fn assert_same_node(adopted: &DurableEngine, copied: &DurableEngine) {
    assert_eq!(adopted.head_info(), copied.head_info());
    assert_eq!(adopted.genesis(), copied.genesis());
    assert_eq!(
        adopted.simulator_block(0).unwrap(),
        copied.simulator_block(0).unwrap()
    );
    let (a, c) = (adopted.head_snapshot(), copied.head_snapshot());
    assert_eq!(a.state_header(), c.state_header());
    let ns = entities().id;
    for n in [0u64, 7, 199] {
        let key = key(n.to_be_bytes());
        let get = |s: &arkiv_db::Snapshot| {
            s.try_get_record(ns, &key, &Projection::ALL, Budget::unlimited())
                .unwrap()
                .unwrap()
                .value
        };
        assert!(get(&a).is_some());
        assert_eq!(get(&a), get(&c));
    }
    for (attribute, value, expected) in [
        ("kind", Value::str("fixture"), 200),
        ("mod", Value::U64(3), 40),
        ("n", Value::U64(150), 1),
    ] {
        let query = Query::all(vec![Literal::of(Term::eq(name(attribute), value))]);
        let count = |s: &arkiv_db::Snapshot| {
            s.try_count(ns, &query, Budget::unlimited())
                .unwrap()
                .unwrap()
                .value
        };
        assert_eq!(count(&a), expected);
        assert_eq!(count(&c), expected);
    }
    for n in [0u8, 19] {
        assert_eq!(
            a.try_host_get(&[n; 32]).unwrap(),
            c.try_host_get(&[n; 32]).unwrap()
        );
    }
}

#[test]
fn adoption_equals_the_copy_and_reopens() {
    for threads in [1, 4] {
        let staged_store = Arc::new(MemoryVersionedStore::new());
        let (engine, staged_genesis) = staged(staged_store.clone(), &format!("eq{threads}"));
        let (identity, genesis, packet) = node(&staged_genesis);
        let target = Arc::new(MemoryVersionedStore::new());
        let copied = committed(
            engine
                .initialize_simulator_from_staged(
                    target.clone(),
                    genesis.clone(),
                    Options::default(),
                    identity.clone(),
                    packet.clone(),
                )
                .unwrap(),
        );
        let info = match engine
            .adopt_staged_simulator(
                genesis.clone(),
                Options::default(),
                identity.clone(),
                packet,
                threads,
            )
            .unwrap()
        {
            Publication::Committed(info) => info,
            other => panic!("{other:?}"),
        };
        assert_eq!(info, copied.head_info());
        assert_eq!(
            DurableEngine::storage_profile(staged_store.as_ref()).unwrap(),
            Some(StorageProfile::Simulator)
        );
        // The relabelled records are exactly the copy's records.
        for (table, key) in [
            (Table::Meta, b"engine/profile".to_vec()),
            (Table::Meta, b"engine/genesis".to_vec()),
            (Table::Meta, b"simulator/identity".to_vec()),
            (Table::Manifests, 0u64.to_be_bytes().to_vec()),
            (Table::Blocks, 0u64.to_be_bytes().to_vec()),
            (Table::BlockIndex, info.commit_id.to_vec()),
        ] {
            let record = target.read_record(table, &key).unwrap();
            assert!(record.is_some(), "{table:?}");
            assert_eq!(staged_store.read_record(table, &key).unwrap(), record);
        }
        // The staged manifest is gone.
        assert_eq!(staged_store.head().unwrap(), target.head().unwrap());
        let adopted = DurableEngine::open_simulator(
            staged_store.clone(),
            genesis.clone(),
            Options::default(),
            identity.clone(),
        )
        .unwrap();
        assert_same_node(&adopted, &copied);
        assert!(
            DurableEngine::open(staged_store.clone(), staged_genesis, Options::default()).is_err()
        );
        drop(adopted);
        // The adopted database keeps producing ordinary publications.
        let mut adopted =
            DurableEngine::open_simulator(staged_store, genesis, Options::default(), identity)
                .unwrap();
        let mut branch = adopted.begin(adopted.head(), None).unwrap();
        branch.try_host_put([99; 32], b"after").unwrap().unwrap();
        let candidate = branch.seal();
        let root = candidate.state_header().root();
        adopted
            .prepare_simulator(
                candidate,
                SimulatorPacket {
                    height: 1,
                    state_root: root,
                    bytes: b"block 1".to_vec(),
                    inclusions: vec![],
                },
            )
            .unwrap();
        assert!(matches!(
            adopted.commit().unwrap(),
            Publication::Committed(_)
        ));
        assert_eq!(adopted.head().sequence(), 1);
    }
}

#[test]
fn only_a_single_exact_bulk_publication_is_eligible() {
    // An incremental import keeps earlier snapshots (and their objects).
    let store = Arc::new(MemoryVersionedStore::new());
    let original = EngineGenesis::new([8; 32], PriceSchedule::FIXTURE);
    let mut stage = committed(
        DurableEngine::initialize(store.clone(), original.clone(), Options::default()).unwrap(),
    );
    for i in 1..=3u8 {
        let mut branch = stage.begin(stage.head(), None).unwrap();
        branch.try_host_put([i; 32], &[i; 40]).unwrap().unwrap();
        commit(&mut stage, branch.seal());
    }
    let mut staged_genesis = EngineGenesis::new([9; 32], PriceSchedule::FIXTURE);
    staged_genesis.initial_state = Some(stage.head_snapshot().state_header());
    let (identity, genesis, packet) = node(&staged_genesis);
    let mut wrong = genesis.clone();
    wrong.initial_state = Some(stage.head_snapshot().state_header());
    let error = stage
        .adopt_staged_simulator(wrong, Options::default(), identity, packet, 2)
        .unwrap_err();
    assert!(
        error.to_string().contains("single height-zero publication"),
        "{error}"
    );
    let stage = DurableEngine::open(store, original, Options::default()).unwrap();
    assert_eq!(stage.head().sequence(), 3);

    // A genesis, identity or packet for another state never relabels.
    let bulk_store = Arc::new(MemoryVersionedStore::new());
    let (bulk, bulk_genesis) = staged(bulk_store.clone(), "eligible");
    let (identity, genesis, packet) = node(&bulk_genesis);

    // The same publication with one record more: the store refuses the
    // relabelling and stays staged.
    let extra_store = Arc::new(MemoryVersionedStore::new());
    build(extra_store.clone(), "extra");
    let mut publication = CommitBatch::new(None, bulk_store.head().unwrap().unwrap());
    for (table, key) in [
        (Table::Meta, b"engine/profile".to_vec()),
        (Table::Meta, b"engine/genesis".to_vec()),
        (Table::Manifests, 0u64.to_be_bytes().to_vec()),
        (Table::BlockIndex, bulk.head_info().commit_id.to_vec()),
    ] {
        let value = bulk_store.read_record(table, &key).unwrap().unwrap();
        publication.records.push((table, key, value));
    }
    publication
        .records
        .push((Table::Meta, b"extra".to_vec(), vec![1]));
    assert_eq!(extra_store.commit(&publication), CommitOutcome::Committed);
    let open_extra = || {
        DurableEngine::open(
            extra_store.clone(),
            bulk_genesis.clone(),
            Options::default(),
        )
    };
    let outcome = open_extra()
        .unwrap()
        .adopt_staged_simulator(
            genesis.clone(),
            Options::default(),
            identity.clone(),
            packet.clone(),
            2,
        )
        .unwrap();
    match outcome {
        Publication::NotCommitted(error) => {
            assert_eq!(error.kind, StorageErrorKind::Incompatible, "{error}")
        }
        other => panic!("{other:?}"),
    }
    open_extra().unwrap();
    let reopen = || {
        DurableEngine::open(bulk_store.clone(), bulk_genesis.clone(), Options::default()).unwrap()
    };
    let mut limits = genesis.clone();
    limits.limits.max_cells -= 1;
    let mut other_identity = identity.clone();
    other_identity.run_id[0] ^= 1;
    let mut other_packet = packet.clone();
    other_packet.state_root = B256::ZERO;
    for (genesis, identity, packet) in [
        (limits, identity.clone(), packet.clone()),
        (genesis.clone(), other_identity, packet.clone()),
        (genesis.clone(), identity.clone(), other_packet),
    ] {
        reopen()
            .adopt_staged_simulator(genesis, Options::default(), identity, packet, 2)
            .unwrap_err();
        assert_eq!(
            DurableEngine::storage_profile(bulk_store.as_ref()).unwrap(),
            Some(StorageProfile::Engine)
        );
    }
    drop(bulk);
    assert!(matches!(
        reopen()
            .adopt_staged_simulator(genesis, Options::default(), identity, packet, 2)
            .unwrap(),
        Publication::Committed(_)
    ));
}

#[test]
fn failures_before_the_relabelling_commit_leave_the_staged_store_unchanged() {
    let inner = Arc::new(MemoryVersionedStore::new());
    let store = Arc::new(FaultyStore::new(inner.clone()));
    let (engine, staged_genesis) = staged(store.clone(), "faults");
    let (identity, genesis, packet) = node(&staged_genesis);
    let reopen =
        || DurableEngine::open(store.clone(), staged_genesis.clone(), Options::default()).unwrap();
    let adopt = |engine: DurableEngine| {
        engine.adopt_staged_simulator(
            genesis.clone(),
            Options::default(),
            identity.clone(),
            packet.clone(),
            3,
        )
    };
    let is_staged =
        || DurableEngine::storage_profile(inner.as_ref()).unwrap() == Some(StorageProfile::Engine);
    drop(engine);

    // A missing reachable object fails the verification walk: no relabelling.
    let root = staged_genesis.initial_header().catalog_root;
    store.override_object(SpaceId::Catalog, root.0, None);
    assert!(adopt(reopen()).is_err());
    assert!(is_staged());
    store.clear_overrides();

    // A definite refusal of the commit itself: unchanged and retryable.
    store.push_commit_fault(CommitFault::NotCommitted(StorageErrorKind::Io));
    assert!(matches!(
        adopt(reopen()).unwrap(),
        Publication::NotCommitted(_)
    ));
    assert!(is_staged());

    // An uncertain commit that did not apply is found staged and retried.
    store.push_commit_fault(CommitFault::IndeterminateNotApplied(StorageErrorKind::Io));
    assert!(matches!(
        adopt(reopen()).unwrap(),
        Publication::Indeterminate(_)
    ));
    assert!(is_staged());

    // An uncertain commit that did apply is found relabelled, and opens as
    // the node database; a second adoption of it is impossible.
    store.push_commit_fault(CommitFault::IndeterminateApplied(StorageErrorKind::Io));
    assert!(matches!(
        adopt(reopen()).unwrap(),
        Publication::Indeterminate(_)
    ));
    assert_eq!(
        DurableEngine::storage_profile(inner.as_ref()).unwrap(),
        Some(StorageProfile::Simulator)
    );
    let adopted = DurableEngine::open_simulator(
        inner.clone(),
        genesis.clone(),
        Options::default(),
        identity.clone(),
    )
    .unwrap();
    assert_eq!(
        adopted.head_snapshot().state_header(),
        staged_genesis.initial_header()
    );
    assert!(
        DurableEngine::open(store.clone(), staged_genesis.clone(), Options::default()).is_err()
    );
}
