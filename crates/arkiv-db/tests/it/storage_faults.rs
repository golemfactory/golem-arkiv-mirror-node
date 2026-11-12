//! Storage failures beneath engine state.
//!
//! Every engine map reads its committed nodes through a loader. A memory
//! engine never has a stored node; here a state is staged
//! (`Snapshot::staged_objects`), kept in a test object store, and reopened
//! from its state header with the hidden `Engine::from_state` test aid, so
//! every read loads nodes lazily through a reader this file controls: it
//! counts reads, fails every read after a chosen number with a chosen
//! error kind, and can lose or damage single objects.
//!
//! On a snapshot a failed, missing or corrupt node surfaces as the storage
//! error, never as an absent record or a rejection. On a branch it also
//! poisons the branch and records no receipt.
//!
//! The unit test `engine::tests` checks that a failed branch operation
//! leaves the branch's state as it was before the operation. What this file
//! does not establish: durability, caching or manifests.

use std::{ops::Bound, sync::Arc};

use crate::trie_support::reference;
use alloy_primitives::{B256, keccak256};
use alloy_trie::EMPTY_ROOT_HASH;
use arkiv_db::{
    BlockNumber, Branch, Budget, Engine, EngineError, Limits, MapKind, OwnerId, PriceSchedule,
    Projection, Query, Snapshot, StagedObject, StateHeader, StorageError, StorageErrorKind, Term,
    TypeTag, Value, WriteOp, try_expire_records,
};
use arkiv_db_storage::{
    testing::ReadScope,
    versioned::{SpaceId, VersionedStore},
};

use crate::support::{
    object_store::{ObjectStore, reopen, stage},
    vocab::{
        ALICE, BOB, add_namespace, apply, attr, begin, create_namespace, create_record, delete,
        delete_record, field, key, name, ns, patch_record, publish, put, transfer,
    },
};

fn record(namespace: u64, record: &[u8], kind: &str, height: u64) -> WriteOp {
    create_record(
        ns(namespace),
        record,
        vec![
            (name("kind"), attr(Value::str(kind))),
            (name("h"), attr(Value::U64(height))),
            (name("note"), field(Value::bytes(b"note"))),
        ],
    )
}

/// Two namespaces with raw and typed records and two host entries.
fn memory_engine() -> Engine {
    let mut engine = Engine::new(PriceSchedule::FIXTURE);
    let mut branch = begin(&engine, None);
    for text in ["alpha", "beta"] {
        add_namespace(&mut branch, ALICE, text);
    }
    for index in 0u8..12 {
        let raw = [b'k', b'0' + index / 10, b'0' + index % 10];
        apply(&mut branch, ALICE, put(ns(1), &raw, &[index; 3]));
        let kind = ["red", "rose", "ruby"][usize::from(index % 3)];
        let op = record(1, &[b'r', index], kind, u64::from(index % 4) + 2);
        apply(&mut branch, ALICE, op);
    }
    apply(&mut branch, ALICE, record(2, b"other", "red", 3));
    branch.try_host_put([1; 32], b"due").unwrap().unwrap();
    branch.try_host_put([2; 32], b"total").unwrap().unwrap();
    publish(&mut engine, branch);
    engine
}

/// The memory engine's head, staged into a fresh store and reopened.
fn reopened() -> (Engine, Engine, Arc<ObjectStore>) {
    let memory = memory_engine();
    let head = memory.head_snapshot();
    let (store, engine) = reopen(&head.state_header(), head.staged_objects());
    (memory, engine, store)
}

/// A read of a snapshot, returning its outcome as debug text.
type SnapshotRead = fn(&Snapshot) -> Result<String, StorageError>;

fn text<T: std::fmt::Debug>(result: Result<T, StorageError>) -> Result<String, StorageError> {
    result.map(|value| format!("{value:?}"))
}

fn traced_budget() -> Budget {
    Budget::unlimited().traced()
}

fn red() -> Query {
    Query::term(Term::eq(name("kind"), Value::str("red")))
}

fn mixed() -> Query {
    Query {
        groups: vec![
            vec![arkiv_db::Literal::of(Term::starts_with(name("kind"), "r"))],
            vec![arkiv_db::Literal::not(Term::range(
                name("h"),
                Bound::Included(Value::U64(3)),
                Bound::Unbounded,
            ))],
        ],
    }
}

/// Every metered read of the fixture and the statistics, traced, so the
/// debug text holds values, receipts with ledgers and physical traces.
fn snapshot_reads() -> Vec<(&'static str, SnapshotRead)> {
    vec![
        ("get", |s| text(s.try_get(ns(1), b"k07", traced_budget()))),
        ("get absent", |s| {
            text(s.try_get(ns(1), b"k99", traced_budget()))
        }),
        ("scan", |s| {
            text(s.try_scan_prefix(ns(1), b"k0", traced_budget()))
        }),
        ("descriptor", |s| {
            text(s.try_descriptor(ns(2), traced_budget()))
        }),
        ("get record", |s| {
            text(s.try_get_record(ns(1), &key([b'r', 4]), &Projection::ALL, traced_budget()))
        }),
        ("query", |s| {
            text(s.try_query(
                ns(1),
                &red(),
                &Projection::KEY_ONLY,
                5,
                None,
                traced_budget(),
            ))
        }),
        ("mixed query", |s| {
            text(s.try_query(ns(1), &mixed(), &Projection::ALL, 20, None, traced_budget()))
        }),
        ("count", |s| {
            text(s.try_count(ns(1), &mixed(), traced_budget()))
        }),
        ("terms", |s| {
            text(s.try_terms(
                ns(1),
                &name("h"),
                TypeTag::U64,
                Bound::Unbounded,
                10,
                traced_budget(),
            ))
        }),
        ("host get", |s| text(s.try_host_get(&[2; 32]))),
        ("host range", |s| text(s.try_host_range(&[0; 32], 10))),
        ("record count", |s| text(s.try_record_count(ns(1)))),
        ("term count", |s| text(s.try_term_count(ns(1)))),
        ("index consistent", |s| text(s.try_index_consistent(ns(1)))),
    ]
}

fn reads(snapshot: &Snapshot) -> Vec<Result<String, StorageError>> {
    snapshot_reads()
        .into_iter()
        .map(|(_, read)| read(snapshot))
        .collect()
}

#[test]
fn a_state_reopened_from_its_objects_reads_the_same_and_loads_lazily() {
    let (memory, engine, store) = reopened();
    assert_eq!(store.stats().object_reads(), 0, "opening reads nothing");
    let original = memory.head_snapshot();
    let snapshot = engine.head_snapshot();
    assert_eq!(snapshot.state_root(), original.state_root());
    assert_eq!(snapshot.state_header(), original.state_header());
    assert_eq!(store.stats().object_reads(), 0, "the root needs no read");
    // Values, receipts and physical node visits: every Debug field.
    let expected = reads(&original);
    assert!(expected.iter().all(Result::is_ok));
    assert_eq!(reads(&snapshot), expected);
    let after_one_pass = store.stats().object_reads();
    assert!(after_one_pass > 0, "reads load nodes");
    // Nothing is cached in the handles: a second pass loads again, and
    // answers (visits included) the same.
    assert_eq!(reads(&snapshot), expected);
    assert_eq!(store.stats().object_reads(), 2 * after_one_pass);
    // Nothing of a memory state is from the store; nothing of an unchanged
    // reopened state needs staging.
    assert!(snapshot.staged_objects().is_empty());
}

#[test]
fn snapshot_reads_fail_with_storage_errors_and_recover_after_the_store_heals() {
    let (memory, engine, store) = reopened();
    let snapshot = engine.head_snapshot();
    let expected = reads(&memory.head_snapshot());
    for kind in [StorageErrorKind::Io, StorageErrorKind::Full] {
        store.fail_reads_after(ReadScope::AllObjects, 0, kind);
        for (index, got) in reads(&snapshot).into_iter().enumerate() {
            // Statistics that need no node (none here) could still answer;
            // every read below touches the catalog.
            let error = got.expect_err("every read loads a node first");
            assert_eq!(error.kind, kind, "read {index}");
        }
        store.clear_read_faults();
        assert_eq!(reads(&snapshot), expected, "the snapshot is not poisoned");
    }
    // Failing the k-th read of each read, for every k below what it needs
    // and with either kind, is that error, never another answer.
    for ((what, read), want) in snapshot_reads().into_iter().zip(&expected) {
        let before = store.stats().object_reads();
        assert_eq!(&read(&snapshot), want, "{what}");
        let needed = store.stats().object_reads() - before;
        assert!(needed > 0, "{what} loads nodes");
        for kind in [StorageErrorKind::Io, StorageErrorKind::Full] {
            for allowance in 0..needed {
                store.fail_reads_after(ReadScope::AllObjects, allowance, kind);
                let got = read(&snapshot);
                store.clear_read_faults();
                assert_eq!(
                    got.map_err(|error| error.kind),
                    Err(kind),
                    "{what} failing after {allowance} of {needed} loads"
                );
            }
        }
    }
}

#[test]
fn missing_and_corrupt_objects_are_availability_errors() {
    let memory = memory_engine();
    let head = memory.head_snapshot();
    let objects = head.staged_objects();
    let header = head.state_header();
    // Namespace 2's objects: its row map is one leaf (one record).
    let ns2: Vec<_> = objects
        .iter()
        .filter(|object| object.space == SpaceId::Namespace(2))
        .map(|object| (object.space, object.id, object.bytes.clone()))
        .collect();
    assert!(!ns2.is_empty());
    for (space, id, bytes) in ns2 {
        for damage in ["missing", "corrupt"] {
            let (store, engine) = reopen(&header, objects.clone());
            let damaged = match damage {
                "missing" => None,
                _ => {
                    let mut damaged = bytes.clone();
                    let last = damaged.len() - 1;
                    damaged[last] ^= 1;
                    Some(damaged)
                }
            };
            store.override_object(space, id, damaged);
            let snapshot = engine.head_snapshot();
            // Namespace 1 never reads namespace 2's objects.
            assert_eq!(reads(&snapshot), reads(&memory.head_snapshot()));
            // A full walk of namespace 2 reaches the damaged object.
            let walk = snapshot.try_query(
                ns(2),
                &Query::match_all(),
                &Projection::ALL,
                10,
                None,
                traced_budget(),
            );
            let consistent = snapshot.try_index_consistent(ns(2));
            let failures: Vec<StorageErrorKind> = [walk.err(), consistent.err()]
                .into_iter()
                .flatten()
                .map(|error| error.kind)
                .collect();
            let kind = match damage {
                "missing" => StorageErrorKind::Missing,
                _ => StorageErrorKind::Corrupt,
            };
            assert!(
                !failures.is_empty() && failures.iter().all(|got| *got == kind),
                "{damage} {id:02x?}: {failures:?}"
            );
        }
    }
}

/// An operation on a branch, returning its outcome as debug text.
type BranchCall = fn(&mut Branch) -> Result<String, StorageError>;

fn exec(branch: &mut Branch, actor: OwnerId, op: WriteOp) -> Result<String, StorageError> {
    text(branch.try_execute(actor, op, traced_budget()))
}

fn calls() -> Vec<(&'static str, BranchCall)> {
    vec![
        ("put", |branch| {
            exec(branch, ALICE, put(ns(1), b"k03", b"new"))
        }),
        ("delete", |branch| {
            exec(branch, ALICE, delete(ns(1), b"k04"))
        }),
        ("create record", |branch| {
            exec(branch, ALICE, record(1, b"new", "rust", 9))
        }),
        ("patch record", |branch| {
            let set = vec![(name("kind"), attr(Value::I64(-1)))];
            let op = patch_record(ns(1), [b'r', 5], set, vec![name("h")]);
            exec(branch, ALICE, op)
        }),
        ("delete record (host)", |branch| {
            let op = delete_record(ns(1), [b'r', 6]);
            text(branch.try_execute_host(op, traced_budget()))
        }),
        ("create namespace", |branch| {
            exec(branch, BOB, create_namespace("gamma"))
        }),
        ("transfer", |branch| {
            exec(branch, ALICE, transfer(ns(2), 0, BOB))
        }),
        ("unauthorized put", |branch| {
            exec(branch, BOB, put(ns(1), b"k", b"v"))
        }),
        ("estimate", |branch| {
            text(branch.try_estimate(ALICE, record(1, b"est", "red", 2), traced_budget()))
        }),
        ("host put", |branch| {
            text(branch.try_host_put([1; 32], b"changed"))
        }),
        ("host remove", |branch| {
            text(branch.try_host_remove(&[2; 32]))
        }),
        ("query", |branch| {
            let query = Query::term(Term::starts_with(name("kind"), "ru"));
            text(branch.try_query(
                ns(1),
                &query,
                &Projection::ALL,
                10,
                None,
                Budget::unlimited(),
            ))
        }),
        ("expiry sweep", |branch| {
            text(try_expire_records(
                branch,
                ns(1),
                &name("h"),
                BlockNumber(3),
                Budget::unlimited(),
                Budget::unlimited(),
            ))
        }),
    ]
}

/// Every later call on a poisoned branch fails, whatever the store does.
fn assert_poisoned(branch: &mut Branch, context: &str) {
    assert!(branch.is_poisoned(), "{context}");
    for (what, call) in calls() {
        let error = call(branch).expect_err(what);
        assert!(
            error.detail.contains("poisoned"),
            "{context}: {what}: {error}"
        );
    }
    assert!(branch.try_get(ns(1), b"k01", Budget::unlimited()).is_err());
    assert!(branch.try_namespace_count().is_err());
    assert_eq!(
        branch.try_checkpoint(),
        Err(EngineError::Poisoned),
        "{context}"
    );
    assert_eq!(
        branch.try_state_root(),
        Err(EngineError::Poisoned),
        "{context}"
    );
    assert_eq!(
        branch.set_pricing(PriceSchedule::FIXTURE.with_id(9)),
        Err(EngineError::Poisoned),
        "{context}"
    );
}

#[test]
fn a_storage_error_poisons_the_branch_and_records_nothing() {
    let (memory, engine, store) = reopened();
    for (what, call) in calls() {
        // What the memory engine does: the reference outcome.
        let mut reference = begin(&memory, None);
        let want = call(&mut reference).expect("memory engines never fail");
        let want_receipts = reference.receipts().to_vec();
        let want_spent = reference.spent();

        // Enough reads: the same outcome, receipts included.
        let mut branch = begin(&engine, None);
        let before = store.stats().object_reads();
        assert_eq!(call(&mut branch).as_ref(), Ok(&want), "{what}");
        let needed = store.stats().object_reads() - before;
        assert!(needed > 0, "{what} loads nodes");
        assert_eq!(branch.receipts(), want_receipts, "{what}");
        assert_eq!(branch.spent(), want_spent, "{what}");
        assert!(branch.try_seal().is_ok(), "{what}");

        // One read short at every point: an error and a poisoned branch.
        for allowance in 0..needed {
            let mut branch = begin(&engine, None);
            let checkpoint = branch.try_checkpoint().unwrap();
            store.fail_reads_after(ReadScope::AllObjects, allowance, StorageErrorKind::Io);
            let error = call(&mut branch).expect_err(what);
            store.clear_read_faults();
            let context = format!("{what} failing after {allowance} of {needed} reads");
            assert_eq!(error.kind, StorageErrorKind::Io, "{context}");
            // The failed operation recorded nothing and spent nothing. A
            // call made of several operations (the expiry sweep) keeps the
            // receipts of those that completed before it, as the memory
            // engine issued them; the poisoned branch can never be sealed.
            let kept = branch.receipts();
            assert!(kept.len() <= want_receipts.len(), "{context}");
            assert_eq!(kept, &want_receipts[..kept.len()], "{context}");
            let kept_spent: u64 = kept.iter().map(|receipt| receipt.spent.units()).sum();
            assert_eq!(branch.spent().units(), kept_spent, "{context}: spend");
            if what != "expiry sweep" {
                assert!(kept.is_empty(), "{context}: a single operation");
            }
            assert_poisoned(&mut branch, &context);
            assert_eq!(
                branch.rollback_to(checkpoint),
                Err(EngineError::Poisoned),
                "{context}"
            );
            assert_eq!(branch.try_seal().err(), Some(EngineError::Poisoned));
        }
    }
    // The published state is untouched by all of it.
    assert_eq!(
        engine.head_snapshot().state_root(),
        memory.head_snapshot().state_root()
    );
}

/// The encoded single-leaf catalog node for 8-byte key `id` and `value`:
/// `[hex_prefix(16 nibbles, leaf), value]`.
fn single_leaf(id: u64, value: &[u8]) -> Vec<u8> {
    let mut path = vec![0x20];
    path.extend(id.to_be_bytes());
    reference::list(&[reference::string(&path), reference::string(value)])
}

/// A catalog leaf for namespace `descriptor_id` with the given map handles.
fn catalog_value(descriptor_id: u64, next_record: u64, handles: [(B256, u64); 4]) -> Vec<u8> {
    let mut leaf = descriptor_id.to_be_bytes().to_vec();
    leaf.extend(5u32.to_be_bytes());
    leaf.extend(b"alpha");
    leaf.extend(ALICE.as_bytes());
    leaf.extend(0u64.to_be_bytes());
    leaf.extend(next_record.to_be_bytes());
    for (root, len) in handles {
        leaf.extend(root.0);
        leaf.extend(len.to_be_bytes());
    }
    leaf
}

/// A state with one catalog entry under key `id` holding `value`.
fn crafted(store: &Arc<ObjectStore>, id: u64, value: Vec<u8>) -> Result<Engine, StorageError> {
    let node = single_leaf(id, &value);
    let root = keccak256(&node);
    let object = StagedObject {
        space: SpaceId::Catalog,
        map: MapKind::Catalog,
        id: root.0,
        bytes: node,
    };
    stage(store, vec![object]);
    let header = StateHeader {
        format: arkiv_db::STATE_FORMAT,
        catalog_root: root,
        catalog_len: 1,
        next_namespace: id + 1,
        pricing: PriceSchedule::FIXTURE,
        host_root: EMPTY_ROOT_HASH,
        host_len: 0,
    };
    Engine::from_state(&header, Limits::DEFAULT, store.clone())
}

/// One namespace where `first` is created and deleted again, so `survivor`
/// has id 2.
fn one_survivor(first: &[u8], survivor: &[u8]) -> Snapshot {
    let mut engine = Engine::new(PriceSchedule::FIXTURE);
    let mut branch = begin(&engine, None);
    add_namespace(&mut branch, ALICE, "alpha");
    for bytes in [first, survivor] {
        apply(&mut branch, ALICE, record(1, bytes, "red", 2));
    }
    apply(&mut branch, ALICE, delete_record(ns(1), first));
    publish(&mut engine, branch);
    engine.head_snapshot()
}

#[test]
fn leaves_that_contradict_their_keys_are_corrupt() {
    // `a` bound to id 2 in one state; id 2's row holds `b` in another.
    let with_a = one_survivor(b"x", b"a");
    let with_b = one_survivor(b"x", b"b");
    let roots_a = with_a.try_namespace_roots(ns(1)).unwrap().unwrap();
    let roots_b = with_b.try_namespace_roots(ns(1)).unwrap().unwrap();
    let (store, _) = reopen(&with_a.state_header(), with_a.staged_objects());
    stage(&store, with_b.staged_objects());

    // A catalog leaf describing namespace 1, stored under key 2.
    let honest = catalog_value(
        1,
        3,
        [roots_a.records, roots_a.keys, roots_a.rows, roots_a.terms],
    );
    let engine = crafted(&store, 2, honest.clone()).unwrap();
    let snapshot = engine.head_snapshot();
    for error in [
        snapshot.try_descriptor(ns(2), traced_budget()).err(),
        snapshot.try_get(ns(2), b"k", traced_budget()).err(),
        snapshot.try_record_count(ns(2)).err(),
    ] {
        assert_eq!(
            error.map(|error| error.kind),
            Some(StorageErrorKind::Corrupt)
        );
    }
    // The same leaf under its own key reads fine.
    let engine = crafted(&store, 1, honest).unwrap();
    let got = engine
        .head_snapshot()
        .try_get_record(ns(1), &key(b"a"), &Projection::KEY_ONLY, traced_budget())
        .unwrap()
        .unwrap();
    assert_eq!(got.value.unwrap().key, Some(key(b"a")));

    // `a`'s binding (id 2) from one state, the rows (id 2 holds `b`) from
    // the other: the row read by key holds another key.
    let mixed = catalog_value(
        1,
        3,
        [roots_a.records, roots_a.keys, roots_b.rows, roots_a.terms],
    );
    let engine = crafted(&store, 1, mixed).unwrap();
    let error = engine
        .head_snapshot()
        .try_get_record(ns(1), &key(b"a"), &Projection::KEY_ONLY, traced_budget())
        .unwrap_err();
    assert_eq!(error.kind, StorageErrorKind::Corrupt, "{error}");

    // A leaf whose key and row counts differ does not decode.
    let (root, _) = roots_a.keys;
    let uneven = catalog_value(
        1,
        3,
        [roots_a.records, (root, 2), roots_a.rows, roots_a.terms],
    );
    let engine = crafted(&store, 1, uneven).unwrap();
    let error = engine
        .head_snapshot()
        .try_descriptor(ns(1), traced_budget())
        .unwrap_err();
    assert_eq!(error.kind, StorageErrorKind::Corrupt, "{error}");
}

/// A row leaf for key `a` with one cell `c` of kind `kind` (1 attribute, 2
/// field) holding `value` (`tag ‖ payload`), encoded by hand: `u8 len ‖
/// key ‖ u32 cell count ‖ (u32 len ‖ name ‖ kind ‖ value)`.
fn row_value(kind: u8, value: &[u8]) -> Vec<u8> {
    let mut leaf = vec![1, b'a'];
    leaf.extend(1u32.to_be_bytes());
    leaf.extend(1u32.to_be_bytes());
    leaf.push(b'c');
    leaf.push(kind);
    leaf.extend(value);
    leaf
}

#[test]
fn rows_whose_attributes_cannot_be_indexed_are_corrupt() {
    // An honest namespace with one record `a` (id 1) holding only a field,
    // so its term map is empty; its row map is replaced below.
    let mut engine = Engine::new(PriceSchedule::FIXTURE);
    let mut branch = begin(&engine, None);
    add_namespace(&mut branch, ALICE, "alpha");
    let cells = vec![(name("c"), field(Value::bytes(b"b")))];
    apply(&mut branch, ALICE, create_record(ns(1), "a", cells));
    publish(&mut engine, branch);
    let honest = engine.head_snapshot();
    let roots = honest.try_namespace_roots(ns(1)).unwrap().unwrap();
    assert_eq!(roots.terms, (EMPTY_ROOT_HASH, 0));

    let bytes_value = [&[3u8, 0, 0, 0, 1][..], b"b"].concat();
    let length = Value::INDEXED_LEN_CEILING + 1;
    let long_str = [
        &[2u8][..],
        &(length as u32).to_be_bytes(),
        &vec![b'x'; length],
    ]
    .concat();
    let cases = [
        ("bytes field", 2, bytes_value.clone(), true),
        ("long str field", 2, long_str.clone(), true),
        ("bytes attribute", 1, bytes_value, false),
        ("long str attribute", 1, long_str, false),
    ];
    for (what, kind, value, readable) in cases {
        let (store, _) = reopen(&honest.state_header(), honest.staged_objects());
        // A single-leaf row map: id 1 → the crafted row.
        let node = single_leaf(1, &row_value(kind, &value));
        let rows_root = keccak256(&node);
        let object = StagedObject {
            space: SpaceId::Namespace(1),
            map: MapKind::Rows,
            id: rows_root.0,
            bytes: node,
        };
        stage(&store, vec![object]);
        let leaf = catalog_value(
            1,
            2,
            [roots.records, roots.keys, (rows_root, 1), roots.terms],
        );
        let engine = crafted(&store, 1, leaf).unwrap();
        let snapshot = engine.head_snapshot();
        let read = snapshot.try_get_record(ns(1), &key(b"a"), &Projection::ALL, traced_budget());
        if readable {
            // The construction is sound: as a field, the same value reads.
            let row = read.unwrap().unwrap().value.unwrap();
            assert_eq!(row.key, Some(key(b"a")), "{what}");
            continue;
        }
        assert_eq!(
            read.map(|_| ()).map_err(|error| error.kind),
            Err(StorageErrorKind::Corrupt),
            "{what}"
        );
        // Deleting or patching the record would build the attribute's
        // terms: the row load fails first, and the branch is poisoned.
        let set = vec![(name("d"), attr(Value::U64(1)))];
        let writes = [
            delete_record(ns(1), "a"),
            patch_record(ns(1), "a", set, vec![name("c")]),
        ];
        for op in writes {
            let mut branch = begin(&engine, None);
            let error = branch.try_execute(ALICE, op, traced_budget()).unwrap_err();
            assert_eq!(error.kind, StorageErrorKind::Corrupt, "{what}: {error}");
            assert!(branch.is_poisoned(), "{what}");
            assert!(branch.receipts().is_empty(), "{what}");
        }
    }
}

#[test]
fn invalid_state_headers_are_refused() {
    let head = memory_engine().head_snapshot();
    let (store, _) = reopen(&head.state_header(), head.staged_objects());
    let mut header = head.state_header();
    header.format = 999;
    let error = Engine::from_state(&header, Limits::DEFAULT, store.clone()).unwrap_err();
    assert_eq!(error.kind, StorageErrorKind::Incompatible);
    let mut header = head.state_header();
    header.host_len = 0;
    let error = Engine::from_state(&header, Limits::DEFAULT, store.clone()).unwrap_err();
    assert_eq!(error.kind, StorageErrorKind::Corrupt);
    let mut header = head.state_header();
    header.catalog_root = EMPTY_ROOT_HASH;
    let error = Engine::from_state(&header, Limits::DEFAULT, store).unwrap_err();
    assert_eq!(error.kind, StorageErrorKind::Corrupt);
    // The header codec refuses the same shapes.
    let mut header = head.state_header();
    header.catalog_len = 0;
    let bytes = arkiv_db::encode(&header);
    assert!(arkiv_db::decode_exact::<StateHeader>(&bytes).is_err());
    let honest = head.state_header();
    assert_eq!(
        arkiv_db::decode_exact::<StateHeader>(&arkiv_db::encode(&honest)),
        Ok(honest)
    );
}
