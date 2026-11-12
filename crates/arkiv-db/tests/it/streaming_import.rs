//! Interrupted staging never publishes and restarts cleanly: the copy of a
//! staged state into a node database, and the bulk build.
use arkiv_db::{
    Budget, Limits, Literal, PriceSchedule, Projection, Query, Term, Value,
    durable::{BulkGenesisBuilder, DurableEngine, EngineGenesis, Options},
};
use arkiv_db_memory::MemoryVersionedStore;
use arkiv_db_storage::{
    testing::{CommitFault, FaultyStore},
    versioned::{StorageErrorKind, Table, VersionedStore},
};
use std::sync::Arc;

use crate::support::{
    durable::{commit, committed, entities, node},
    vocab::{OWNER, add_namespace, apply, attr, create_record, key, name, ns},
};

/// A store over `target` that refuses `stage_objects` call number `fail_on`
/// (from zero) with `NotCommitted(Io)`. Staging is the only write before the
/// publishing commit, so the call number is the write number.
fn interrupted(target: Arc<MemoryVersionedStore>, fail_on: usize) -> Arc<dyn VersionedStore> {
    let store = FaultyStore::new(target);
    store.skip_writes(fail_on as u64);
    store.push_commit_fault(CommitFault::NotCommitted(StorageErrorKind::Io));
    Arc::new(store)
}

/// A fresh store that counts its `stage_objects` calls.
fn counting() -> Arc<FaultyStore<MemoryVersionedStore>> {
    Arc::new(FaultyStore::new(MemoryVersionedStore::new()))
}

#[test]
fn incomplete_copy_is_headless_and_restarts_idempotently() {
    let source = Arc::new(MemoryVersionedStore::new());
    let original = EngineGenesis::new([8; 32], PriceSchedule::FIXTURE);
    let mut stage =
        committed(DurableEngine::initialize(source, original, Options::default()).unwrap());
    let mut branch = stage.begin(stage.head(), None).unwrap();
    for i in 0..32u8 {
        branch.try_host_put([i; 32], &[i; 64]).unwrap().unwrap();
    }
    commit(&mut stage, branch.seal());
    let mut staged = EngineGenesis::new([7; 32], PriceSchedule::FIXTURE);
    staged.initial_state = Some(stage.head_snapshot().state_header());
    let (identity, genesis, packet) = node(&staged);
    let copy = |target: Arc<dyn VersionedStore>, options: Options| {
        stage.initialize_simulator_from_staged(
            target,
            genesis.clone(),
            options,
            identity.clone(),
            packet.clone(),
        )
    };
    let target = Arc::new(MemoryVersionedStore::new());
    let small = Options {
        prepare_bytes: 4096,
        prepare_objects: 1,
        ..Options::default()
    };

    assert!(copy(interrupted(target.clone(), 2), small).is_err());
    assert_eq!(target.head().unwrap(), None);
    assert!(
        target
            .stats()
            .spaces
            .values()
            .map(|s| s.objects_written)
            .sum::<u64>()
            > 0
    );
    assert!(
        DurableEngine::open_simulator(
            target.clone(),
            genesis.clone(),
            Options::default(),
            identity.clone()
        )
        .is_err()
    );

    let final_engine = committed(copy(target, Options::default()).unwrap());
    assert_eq!(
        final_engine.head_snapshot().state_header(),
        stage.head_snapshot().state_header()
    );
}

#[test]
fn shared_postings_survive_copy_interruptions_and_reopen() {
    let source = Arc::new(MemoryVersionedStore::new());
    let original = EngineGenesis::new([12; 32], PriceSchedule::FIXTURE);
    let mut stage =
        committed(DurableEngine::initialize(source, original, Options::default()).unwrap());
    let mut branch = stage.begin(stage.head(), None).unwrap();
    for ordinal in 1..=2 {
        let namespace = add_namespace(&mut branch, OWNER, &format!("copy{ordinal}"));
        for i in 0..32u8 {
            // Both existence and value terms share the same tree, and the
            // second namespace has identical posting roots.
            let cells = ["first", "second"]
                .into_iter()
                .map(|cell| (name(cell), attr(Value::U64(7))))
                .collect();
            apply(&mut branch, OWNER, create_record(namespace, [i], cells));
        }
    }
    commit(&mut stage, branch.seal());
    let mut staged = EngineGenesis::new([13; 32], PriceSchedule::FIXTURE);
    staged.initial_state = Some(stage.head_snapshot().state_header());
    let (identity, genesis, packet) = node(&staged);
    let options = Options {
        prepare_bytes: 4096,
        prepare_objects: 2,
        ..Options::default()
    };
    let copy = |target: Arc<dyn VersionedStore>| {
        stage.initialize_simulator_from_staged(
            target,
            genesis.clone(),
            options.clone(),
            identity.clone(),
            packet.clone(),
        )
    };
    let complete = counting();
    committed(copy(complete.clone()).unwrap());
    let calls = complete.injected().stage_calls as usize;
    assert!(calls > 2);
    // Include the final flush, after the repeated posting references were seen.
    for fail_on in [0, calls / 2, calls - 1] {
        let target = Arc::new(MemoryVersionedStore::new());
        assert!(copy(interrupted(target.clone(), fail_on)).is_err());
        assert_eq!(target.head().unwrap(), None);
        assert!(
            target
                .read_record(Table::Meta, b"engine/genesis")
                .unwrap()
                .is_none()
        );
        committed(copy(target.clone()).unwrap());
        let copied = DurableEngine::open_simulator(
            target,
            genesis.clone(),
            options.clone(),
            identity.clone(),
        )
        .unwrap();
        assert_eq!(
            copied.head_snapshot().state_header(),
            stage.head_snapshot().state_header()
        );
        for namespace in [ns(1), ns(2)] {
            for cell in ["first", "second"] {
                let query = Query::all(vec![Literal::of(Term::eq(name(cell), Value::U64(7)))]);
                assert_eq!(
                    copied
                        .head_snapshot()
                        .try_count(namespace, &query, Budget::unlimited())
                        .unwrap()
                        .unwrap()
                        .value,
                    32
                );
            }
            let key = key([31]);
            let original = stage
                .head_snapshot()
                .try_get_record(namespace, &key, &Projection::ALL, Budget::unlimited())
                .unwrap()
                .unwrap()
                .value;
            let actual = copied
                .head_snapshot()
                .try_get_record(namespace, &key, &Projection::ALL, Budget::unlimited())
                .unwrap()
                .unwrap()
                .value;
            assert_eq!(actual, original);
        }
    }
}

#[test]
fn bulk_stage_interruptions_never_publish_and_clean_rebuild_succeeds() {
    let directory =
        std::env::temp_dir().join(format!("arkiv-bulk-interrupt-{}", std::process::id()));
    std::fs::create_dir(&directory).unwrap();
    let options = Options {
        prepare_objects: 2,
        prepare_bytes: 4096,
        ..Options::default()
    };
    let make = |dir: &str| {
        let mut builder =
            BulkGenesisBuilder::new(&directory.join(dir), entities(), Limits::DEFAULT, 4096)
                .unwrap();
        for n in 0..32u8 {
            builder
                .add_record(key([n]), vec![(name("shared"), attr(Value::U64(7)))])
                .unwrap();
        }
        builder
    };
    let complete = counting();
    let expected = make("complete")
        .finish(complete.clone(), PriceSchedule::FIXTURE, options.clone())
        .unwrap()
        .state_header()
        .clone();
    let calls = complete.injected().stage_calls as usize;
    assert!(calls > 2);
    for (i, fail_on) in [0, calls / 2, calls - 1].into_iter().enumerate() {
        let target = Arc::new(MemoryVersionedStore::new());
        let failing = interrupted(target.clone(), fail_on);
        assert!(
            make(&format!("failed{i}"))
                .finish(failing, PriceSchedule::FIXTURE, options.clone())
                .is_err()
        );
        assert!(target.head().unwrap().is_none());
        assert!(
            target
                .read_record(Table::Meta, b"engine/genesis")
                .unwrap()
                .is_none()
        );
        let build = make(&format!("retry{i}"))
            .finish(target.clone(), PriceSchedule::FIXTURE, options.clone())
            .unwrap();
        assert_eq!(build.state_header(), &expected);
        let mut genesis = EngineGenesis::new([42; 32], PriceSchedule::FIXTURE);
        genesis.initial_state = Some(expected.clone());
        committed(DurableEngine::initialize_bulk(build, genesis.clone(), options.clone()).unwrap());
        assert_eq!(
            DurableEngine::open(target, genesis, options.clone())
                .unwrap()
                .head_snapshot()
                .state_header(),
            expected
        );
    }
    std::fs::remove_dir_all(directory).unwrap();
}
