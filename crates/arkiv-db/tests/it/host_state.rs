//! The engine's host-state map: 32-byte keys bound to values of at most
//! 256 bytes, versioned with the rest of the state and invisible to the data
//! plane.
//!
//! Expected answers come from a `BTreeMap` model of the host writes and from
//! the same workload run on an engine without host state; no cost is
//! asserted beyond equality, because the claim is that host state changes
//! nothing.

use std::{collections::BTreeMap, ops::Bound};

use arkiv_db::{
    BlockNumber, Branch, Budget, Cursor, Engine, EngineError, HOST_VALUE_MAX, HostKey,
    HostStateError, NamespaceId, OwnerId, PriceSchedule, Projection, Query, QueryPage, ReadResult,
    Rejection, TypeTag, Value, WriteOp, expire_records,
};

use crate::support::{
    reader::Reader,
    vocab::{
        ALICE, BOB, add_namespace, apply, attr, begin, create_namespace, create_record, delete,
        delete_record, field, key, name, ns, patch_record, publish, put, rid, transfer,
    },
};

/// A host key: `first` followed by 31 zero bytes.
fn hk(first: u8) -> HostKey {
    let mut key = [0u8; 32];
    key[0] = first;
    key
}

/// A host key spelling `bytes`, zero-padded to 32 bytes.
fn spelled(bytes: &[u8]) -> HostKey {
    let mut key = [0u8; 32];
    key[..bytes.len()].copy_from_slice(bytes);
    key
}

fn host_put(branch: &mut Branch, key: HostKey, value: &[u8]) -> Option<Vec<u8>> {
    branch
        .try_host_put(key, value)
        .expect("in-memory host state never fails")
        .expect("the value fits")
}

fn host_get(branch: &Branch, key: &HostKey) -> Option<Vec<u8>> {
    branch
        .try_host_get(key)
        .expect("in-memory host state never fails")
}

fn all(reader: &impl Reader) -> Vec<(HostKey, Vec<u8>)> {
    reader
        .try_host_range(&[0; 32], usize::MAX)
        .expect("in-memory host state never fails")
}

fn entries(pairs: &[(HostKey, &[u8])]) -> Vec<(HostKey, Vec<u8>)> {
    pairs
        .iter()
        .map(|(key, value)| (*key, value.to_vec()))
        .collect()
}

fn add_record(branch: &mut Branch, namespace: NamespaceId, record: &[u8], height: u64) {
    let cells = vec![(name("h"), attr(Value::U64(height)))];
    apply(branch, ALICE, create_record(namespace, record, cells));
}

#[test]
fn puts_replace_and_removes_return_the_previous_value() {
    let engine = Engine::new(PriceSchedule::FIXTURE);
    let mut branch = begin(&engine, None);
    assert_eq!(host_put(&mut branch, hk(1), b"one"), None);
    assert_eq!(host_get(&branch, &hk(1)), Some(b"one".to_vec()));
    assert_eq!(host_put(&mut branch, hk(1), b"uno"), Some(b"one".to_vec()));
    assert_eq!(host_get(&branch, &hk(1)), Some(b"uno".to_vec()));
    assert_eq!(branch.try_host_remove(&hk(1)), Ok(Some(b"uno".to_vec())));
    assert_eq!(branch.try_host_remove(&hk(1)), Ok(None));
    assert_eq!(host_get(&branch, &hk(1)), None);

    // The empty value and a value of exactly HOST_VALUE_MAX bytes fit.
    assert_eq!(HOST_VALUE_MAX, 256);
    assert_eq!(host_put(&mut branch, hk(2), b""), None);
    assert_eq!(host_get(&branch, &hk(2)), Some(Vec::new()));
    let largest = [0x5a; 256];
    assert_eq!(host_put(&mut branch, hk(3), &largest), None);
    assert_eq!(host_get(&branch, &hk(3)), Some(largest.to_vec()));

    // Nothing the host writes is a receipt or a charge.
    assert!(branch.receipts().is_empty());
    assert_eq!(branch.spent().units(), 0);
}

#[test]
fn oversize_values_are_refused_and_change_nothing() {
    let engine = Engine::new(PriceSchedule::FIXTURE);
    let mut branch = begin(&engine, None);
    add_namespace(&mut branch, ALICE, "alpha");
    add_record(&mut branch, ns(1), b"a", 1);
    add_record(&mut branch, ns(1), b"b", 2);
    host_put(&mut branch, hk(1), b"kept");
    host_put(&mut branch, hk(2), b"other");
    let before = all(&branch);

    // A branch cursor issued now stays valid across the refused writes.
    let query = Query::match_all();
    let page = branch
        .query(
            ns(1),
            &query,
            &Projection::ID_ONLY,
            1,
            None,
            Budget::unlimited(),
        )
        .unwrap();
    let cursor = page.value.next.expect("a second record follows");

    for len in [257, 300, 4096] {
        let value = vec![0x33; len];
        assert_eq!(
            branch.try_host_put(hk(1), &value),
            Ok(Err(HostStateError::ValueTooLarge { len }))
        );
        assert_eq!(
            branch.try_host_put(hk(9), &value),
            Ok(Err(HostStateError::ValueTooLarge { len }))
        );
    }
    assert_eq!(all(&branch), before);
    assert_eq!(host_get(&branch, &hk(1)), Some(b"kept".to_vec()));
    assert_eq!(host_get(&branch, &hk(9)), None);
    let next = branch.query(
        ns(1),
        &query,
        &Projection::ID_ONLY,
        1,
        Some(&cursor),
        Budget::unlimited(),
    );
    assert!(next.is_ok(), "a refused host write changed the serial");
    assert_eq!(
        HostStateError::ValueTooLarge { len: 257 }.to_string(),
        "host value of 257 bytes exceeds 256 bytes"
    );
}

#[test]
fn ranges_are_bounded_ordered_and_start_at_from() {
    let mut engine = Engine::new(PriceSchedule::FIXTURE);
    let mut branch = begin(&engine, None);
    let mut one_then_one = hk(1);
    one_then_one[31] = 1;
    let mut one_then_two = hk(1);
    one_then_two[31] = 2;
    // Inserted out of order.
    for (key, value) in [
        (hk(0x80), b"80".as_slice()),
        ([0xff; 32], b"ff"),
        (hk(1), b"01"),
        ([0; 32], b"00"),
        (one_then_one, b"0101"),
    ] {
        host_put(&mut branch, key, value);
    }
    let everything = entries(&[
        ([0; 32], b"00"),
        (hk(1), b"01"),
        (one_then_one, b"0101"),
        (hk(0x80), b"80"),
        ([0xff; 32], b"ff"),
    ]);
    assert_eq!(all(&branch), everything);
    assert_eq!(branch.try_host_range(&[0; 32], 5), Ok(everything.clone()));
    assert_eq!(
        branch.try_host_range(&[0; 32], 2),
        Ok(everything[..2].to_vec())
    );
    assert_eq!(
        branch.try_host_range(&hk(1), 2),
        Ok(entries(&[(hk(1), b"01"), (one_then_one, b"0101")]))
    );
    // `from` need not be present.
    assert_eq!(
        branch.try_host_range(&one_then_two, 10),
        Ok(entries(&[(hk(0x80), b"80"), ([0xff; 32], b"ff")]))
    );
    assert_eq!(
        branch.try_host_range(&[0xff; 32], 10),
        Ok(entries(&[([0xff; 32], b"ff")]))
    );
    assert_eq!(branch.try_host_range(&[0; 32], 0), Ok(Vec::new()));

    let id = publish(&mut engine, branch);
    let snapshot = engine.snapshot(id).unwrap();
    assert_eq!(all(&snapshot), everything);
    assert_eq!(
        snapshot.try_host_range(&hk(1), 1),
        Ok(entries(&[(hk(1), b"01")]))
    );
    assert_eq!(snapshot.try_host_range(&hk(0x81), 0), Ok(Vec::new()));
    assert_eq!(
        snapshot.try_host_get(&one_then_one),
        Ok(Some(b"0101".to_vec()))
    );
    assert_eq!(snapshot.try_host_get(&one_then_two), Ok(None));
}

#[test]
fn checkpoints_and_rollbacks_restore_host_state() {
    let engine = Engine::new(PriceSchedule::FIXTURE);
    let mut branch = begin(&engine, None);
    host_put(&mut branch, hk(1), b"a1");
    host_put(&mut branch, hk(3), b"c1");
    let first = branch.checkpoint();
    host_put(&mut branch, hk(1), b"a2");
    host_put(&mut branch, hk(2), b"b2");
    assert_eq!(branch.try_host_remove(&hk(3)), Ok(Some(b"c1".to_vec())));
    let second = branch.checkpoint();
    host_put(&mut branch, hk(4), b"d");
    add_namespace(&mut branch, ALICE, "alpha");

    assert_eq!(branch.rollback_to(second), Ok(0..1));
    assert_eq!(all(&branch), entries(&[(hk(1), b"a2"), (hk(2), b"b2")]));
    assert_eq!(branch.namespace_count(), 0);

    assert_eq!(branch.rollback_to(first), Ok(0..1));
    assert_eq!(all(&branch), entries(&[(hk(1), b"a1"), (hk(3), b"c1")]));
    assert_eq!(
        branch.rollback_to(second),
        Err(EngineError::InvalidCheckpoint)
    );
    // Rolling back to the same checkpoint again restores the same state.
    host_put(&mut branch, hk(5), b"e");
    assert_eq!(branch.rollback_to(first), Ok(0..1));
    assert_eq!(all(&branch), entries(&[(hk(1), b"a1"), (hk(3), b"c1")]));
}

#[test]
fn snapshots_keep_their_own_host_state() {
    let mut engine = Engine::new(PriceSchedule::FIXTURE);

    let mut first = begin(&engine, None);
    host_put(&mut first, hk(1), b"a1");
    host_put(&mut first, hk(2), b"b1");
    let one = publish(&mut engine, first);

    let mut second = engine.begin(one, None).unwrap();
    assert_eq!(host_get(&second, &hk(1)), Some(b"a1".to_vec()));
    host_put(&mut second, hk(1), b"a2");
    second.try_host_remove(&hk(2)).unwrap();
    host_put(&mut second, hk(3), b"c2");
    // A competing candidate on the same base loses and changes nothing.
    let mut competitor = engine.begin(one, None).unwrap();
    host_put(&mut competitor, hk(9), b"lost");
    let two = publish(&mut engine, second);
    assert_eq!(
        engine.publish(&competitor.seal()),
        Err(EngineError::StaleHead {
            expected: one,
            head: two
        })
    );

    // A rolled-back write and a discarded candidate never reach a snapshot.
    let mut third = engine.begin(two, None).unwrap();
    let checkpoint = third.checkpoint();
    host_put(&mut third, hk(4), b"rolled back");
    third.rollback_to(checkpoint).unwrap();
    host_put(&mut third, hk(5), b"e3");
    let three = publish(&mut engine, third);
    let mut discarded = engine.begin(three, None).unwrap();
    host_put(&mut discarded, hk(1), b"discarded");
    drop(discarded);

    let at = |id| all(&engine.snapshot(id).unwrap());
    assert_eq!(at(arkiv_db::SnapshotId::GENESIS), Vec::new());
    assert_eq!(at(one), entries(&[(hk(1), b"a1"), (hk(2), b"b1")]));
    assert_eq!(at(two), entries(&[(hk(1), b"a2"), (hk(3), b"c2")]));
    assert_eq!(
        at(three),
        entries(&[(hk(1), b"a2"), (hk(3), b"c2"), (hk(5), b"e3")])
    );
    assert_eq!(
        engine.head_snapshot().try_host_get(&hk(1)),
        Ok(Some(b"a2".to_vec()))
    );
    assert_eq!(engine.head_snapshot().try_host_get(&hk(4)), Ok(None));
}

#[test]
fn host_writes_advance_the_branch_mutation_serial() {
    let engine = Engine::new(PriceSchedule::FIXTURE);
    let mut branch = begin(&engine, None);
    add_namespace(&mut branch, ALICE, "alpha");
    add_record(&mut branch, ns(1), b"a", 1);
    add_record(&mut branch, ns(1), b"b", 2);
    let query = Query::match_all();
    let cursor = |branch: &Branch| -> Cursor {
        branch
            .query(
                ns(1),
                &query,
                &Projection::ID_ONLY,
                1,
                None,
                Budget::unlimited(),
            )
            .unwrap()
            .value
            .next
            .expect("a second record follows")
    };
    let resume = |branch: &Branch, cursor: &Cursor| {
        branch
            .query(
                ns(1),
                &query,
                &Projection::ID_ONLY,
                1,
                Some(cursor),
                Budget::unlimited(),
            )
            .map(|_| ())
            .map_err(|rejected| rejected.reason)
    };

    let issued = cursor(&branch);
    assert_eq!(resume(&branch, &issued), Ok(()));
    host_put(&mut branch, hk(1), b"x");
    assert_eq!(resume(&branch, &issued), Err(Rejection::CursorMismatch));

    let issued = cursor(&branch);
    host_put(&mut branch, hk(1), b"x");
    assert_eq!(
        resume(&branch, &issued),
        Err(Rejection::CursorMismatch),
        "rewriting the same value is still a write"
    );

    let issued = cursor(&branch);
    assert_eq!(branch.try_host_remove(&hk(7)), Ok(None));
    assert_eq!(
        resume(&branch, &issued),
        Err(Rejection::CursorMismatch),
        "removing an absent key advances the serial like a raw delete"
    );

    // Reads of host state change nothing.
    let issued = cursor(&branch);
    host_get(&branch, &hk(1));
    branch.try_host_range(&[0; 32], 10).unwrap();
    assert_eq!(resume(&branch, &issued), Ok(()));
}

/// One workload step: an owner write, or a host-path write.
enum WorkloadStep {
    Owner(OwnerId, WriteOp),
    Host(WriteOp),
}

fn workload() -> Vec<WorkloadStep> {
    vec![
        WorkloadStep::Owner(ALICE, create_namespace("alpha")),
        WorkloadStep::Owner(ALICE, create_namespace("beta")),
        WorkloadStep::Owner(ALICE, put(ns(1), b"k1", b"v1")),
        WorkloadStep::Owner(ALICE, put(ns(1), b"k2", b"v2")),
        WorkloadStep::Owner(ALICE, put(ns(2), b"k1", b"other")),
        WorkloadStep::Owner(
            ALICE,
            create_record(
                ns(1),
                b"r1",
                vec![
                    (name("h"), attr(Value::U64(5))),
                    (name("t"), attr(Value::str("x"))),
                    (name("f"), field(Value::bytes(&[1, 2]))),
                ],
            ),
        ),
        WorkloadStep::Owner(
            ALICE,
            create_record(
                ns(1),
                b"r2",
                vec![
                    (name("h"), attr(Value::U64(3))),
                    (name("t"), attr(Value::str("y"))),
                ],
            ),
        ),
        WorkloadStep::Owner(
            ALICE,
            create_record(ns(1), b"r3", vec![(name("h"), attr(Value::U64(9)))]),
        ),
        WorkloadStep::Owner(
            ALICE,
            patch_record(
                ns(1),
                b"r2",
                vec![(name("t"), attr(Value::str("z")))],
                Vec::new(),
            ),
        ),
        WorkloadStep::Owner(ALICE, delete(ns(1), b"k2")),
        WorkloadStep::Owner(ALICE, delete_record(ns(1), b"r3")),
        WorkloadStep::Host(patch_record(
            ns(1),
            b"r1",
            vec![(name("g"), attr(Value::I64(-4)))],
            vec![name("f")],
        )),
        WorkloadStep::Owner(ALICE, transfer(ns(2), 0, BOB)),
        // Rejections: unauthorized, duplicate key, oversized raw key, a raw
        // write on the host path, an unknown namespace.
        WorkloadStep::Owner(ALICE, put(ns(2), b"k9", b"no")),
        WorkloadStep::Owner(ALICE, create_record(ns(1), b"r2", Vec::new())),
        WorkloadStep::Owner(ALICE, put(ns(1), &[7; 21], b"")),
        WorkloadStep::Host(put(ns(1), b"k1", b"host")),
        WorkloadStep::Owner(ALICE, put(ns(9), b"k1", b"v")),
        WorkloadStep::Owner(BOB, put(ns(2), b"k3", b"bob")),
    ]
}

/// Host entries whose keys and values spell the workload's own
/// identities: namespace ids, raw keys, record keys, an all-zero and an
/// all-ones key.
fn host_writes(step: usize) -> Vec<(HostKey, Vec<u8>)> {
    let index = u8::try_from(step).unwrap();
    vec![
        (spelled(&[0, 0, 0, 0, 0, 0, 0, 1]), vec![index]),
        (spelled(b"k1"), b"v1".to_vec()),
        (spelled(b"r1"), vec![index; usize::from(index) + 1]),
        ([0; 32], b"alpha".to_vec()),
        ([0xff; 32], vec![0xff; 256]),
        (hk(0x40 + index), b"h".to_vec()),
    ]
}

/// Where the observation of a branch and of a snapshot differ.
struct Probe {
    raw_keys: &'static [&'static [u8]],
    /// Follow the cursors to the last page instead of stopping at the first.
    walks: bool,
    terms: (&'static str, TypeTag),
}

const OF_BRANCH: Probe = Probe {
    raw_keys: &[b"k1", b"k2"],
    walks: false,
    terms: ("h", TypeTag::U64),
};

const OF_SNAPSHOT: Probe = Probe {
    raw_keys: &[b"k1"],
    walks: true,
    terms: ("t", TypeTag::Str),
};

/// Every metered read and statistic of a reader, with query cursors reduced
/// to their positions (cursor contexts name the engine instance).
fn observe(reader: &impl Reader, probe: &Probe) -> Vec<String> {
    let traced = Budget::unlimited().traced();
    let mut seen = vec![format!("{}", reader.namespace_count())];
    for ordinal in 1..=3 {
        let namespace = ns(ordinal);
        for raw in probe.raw_keys {
            seen.push(format!("{:?}", reader.get(namespace, raw, traced)));
        }
        seen.push(format!("{:?}", reader.scan_prefix(namespace, b"", traced)));
        seen.push(format!("{:?}", reader.descriptor(namespace, traced)));
        for record in [b"r1", b"r2", b"r3"] {
            let row = reader.get_record(namespace, &key(record), &Projection::ALL, traced);
            seen.push(format!("{row:?}"));
        }
        for query in queries() {
            let mut cursor = None;
            loop {
                let page = reader.query(
                    namespace,
                    &query,
                    &Projection::ALL,
                    1,
                    cursor.as_ref(),
                    traced,
                );
                seen.push(page_view(page.clone()));
                cursor = page.ok().and_then(|page| page.value.next);
                if !probe.walks || cursor.is_none() {
                    break;
                }
            }
            seen.push(format!("{:?}", reader.count(namespace, &query, traced)));
        }
        let (attribute, tag) = probe.terms;
        seen.push(format!(
            "{:?}",
            reader.terms(
                namespace,
                &name(attribute),
                tag,
                Bound::Unbounded,
                10,
                traced
            )
        ));
        seen.push(format!(
            "{:?} {:?} {:?} {:?}",
            reader.record_count(namespace),
            reader.typed_record_count(namespace),
            reader.term_count(namespace),
            reader.index_consistent(namespace)
        ));
    }
    seen
}

fn queries() -> Vec<Query> {
    vec![
        Query::match_all(),
        Query::term(arkiv_db::Term::range(
            name("h"),
            Bound::Included(Value::U64(0)),
            Bound::Unbounded,
        )),
        Query::term(arkiv_db::Term::starts_with(name("t"), "")),
    ]
}

fn page_view(result: ReadResult<QueryPage>) -> String {
    match result {
        Ok(metered) => format!(
            "{:?} next={:?} {:?} {:?}",
            metered.value.rows,
            metered.value.next.as_ref().map(Cursor::after),
            metered.receipt,
            metered.physical
        ),
        Err(rejected) => format!("{rejected:?}"),
    }
}

#[test]
fn the_data_plane_never_sees_host_state() {
    let mut plain = Engine::new(PriceSchedule::FIXTURE);
    let mut hosted = Engine::new(PriceSchedule::FIXTURE);
    let mut left = begin(&plain, Some(100_000));
    let mut right = begin(&hosted, Some(100_000));
    let mut model: BTreeMap<HostKey, Vec<u8>> = BTreeMap::new();
    let traced = Budget::unlimited().traced();
    let mut rejections = Vec::new();

    for (index, step) in workload().into_iter().enumerate() {
        for (key, value) in host_writes(index) {
            host_put(&mut right, key, &value);
            model.insert(key, value);
        }
        if index % 3 == 2 {
            let removed = hk(0x40 + u8::try_from(index - 1).unwrap());
            right.try_host_remove(&removed).unwrap();
            model.remove(&removed);
        }
        let host_before = all(&right);
        let (left_result, right_result) = match step {
            WorkloadStep::Owner(actor, op) => {
                assert_eq!(
                    left.estimate(actor, op.clone(), traced),
                    right.estimate(actor, op.clone(), traced),
                    "estimate of step {index}"
                );
                assert_eq!(all(&right), host_before, "an estimate changed host state");
                (
                    left.execute(actor, op.clone(), traced),
                    right.execute(actor, op, traced),
                )
            }
            WorkloadStep::Host(op) => {
                assert_eq!(
                    left.estimate_host(op.clone(), traced),
                    right.estimate_host(op.clone(), traced),
                    "host estimate of step {index}"
                );
                assert_eq!(all(&right), host_before, "an estimate changed host state");
                (
                    left.execute_host(op.clone(), traced),
                    right.execute_host(op, traced),
                )
            }
        };
        assert_eq!(left_result, right_result, "step {index}");
        if let Err(rejected) = right_result {
            rejections.push((index, rejected.reason));
        }
        assert_eq!(left.receipts(), right.receipts());
        assert_eq!(left.spent(), right.spent());
        assert_eq!(
            observe(&left, &OF_BRANCH),
            observe(&right, &OF_BRANCH),
            "after step {index}"
        );
        let expected: Vec<(HostKey, Vec<u8>)> = model.clone().into_iter().collect();
        assert_eq!(all(&right), expected, "host state after step {index}");
        assert_eq!(
            all(&left),
            Vec::new(),
            "data-plane state leaked into host state"
        );
    }

    // Steps 13 to 17 are rejected, every other step applies.
    assert_eq!(
        rejections,
        vec![
            (13, Rejection::Unauthorized),
            (14, Rejection::AlreadyExists),
            (
                15,
                Rejection::InvalidArgument(arkiv_db::ArgumentFault::KeyTooLong)
            ),
            (
                16,
                Rejection::InvalidArgument(arkiv_db::ArgumentFault::NotHostOperation)
            ),
            (17, Rejection::UnknownNamespace),
        ]
    );

    // The expiry sweep deletes the same records with the same receipts:
    // r2 (height 3, record id 2); r1 (height 5) stays.
    let h = name("h");
    let block = BlockNumber(4);
    let left_expired = expire_records(&mut left, ns(1), &h, block, traced, traced);
    let right_expired = expire_records(&mut right, ns(1), &h, block, traced, traced);
    assert_eq!(left_expired, right_expired);
    let expired: Vec<_> = right_expired
        .unwrap()
        .into_iter()
        .map(|(id, _)| id)
        .collect();
    assert_eq!(expired, vec![rid(2)]);
    assert_eq!(left.receipts(), right.receipts());
    let expected: Vec<(HostKey, Vec<u8>)> = model.into_iter().collect();
    assert_eq!(all(&right), expected);

    let left_sealed = left.seal();
    let right_sealed = right.seal();
    assert_eq!(left_sealed.receipts(), right_sealed.receipts());
    assert_eq!(left_sealed.spent(), right_sealed.spent());
    let left_id = plain.publish(&left_sealed).unwrap();
    let right_id = hosted.publish(&right_sealed).unwrap();
    let left_snapshot = plain.snapshot(left_id).unwrap();
    let right_snapshot = hosted.snapshot(right_id).unwrap();
    assert_eq!(
        observe(&left_snapshot, &OF_SNAPSHOT),
        observe(&right_snapshot, &OF_SNAPSHOT)
    );
    assert_eq!(all(&right_snapshot), expected);
    assert_eq!(all(&left_snapshot), Vec::new());
}
