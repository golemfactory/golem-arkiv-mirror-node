//! Admission bounds on names, raw keys and indexed values, on branches and
//! published snapshots.

use std::ops::Bound;

use arkiv_db::*;

use crate::support::vocab::{
    ALICE, BOB, add_namespace, apply, attr, begin, create_record, delete, field, patch_record,
    publish, put, transfer,
};

fn assert_admission(error: Rejected, reason: Rejection) {
    assert_eq!(error.reason, reason);
    assert_eq!(error.receipt.spent, Cost::ZERO);
    assert_eq!(error.physical, PhysicalTrace::default());
}

#[test]
fn names_are_bounded_and_follow_namespace_history_and_transfers() {
    let name = "n".repeat(20);
    for invalid in [
        "n".repeat(CellName::MAX_LEN + 1),
        "é".repeat(20),
        String::new(),
    ] {
        assert!(NamespaceName::new(&invalid).is_err());
        assert!(CellName::new(&invalid).is_err());
    }
    assert_eq!(CellName::new(&name).unwrap().as_str(), name);
    assert_eq!(NamespaceName::new(&name).unwrap().as_str(), name);
    let mut engine = Engine::new(PriceSchedule::FIXTURE);
    let mut branch = begin(&engine, None);
    let ns = add_namespace(&mut branch, ALICE, &name);
    apply(&mut branch, ALICE, put(ns, b"data", b"kept"));
    let checkpoint = branch.checkpoint();
    let discarded = add_namespace(&mut branch, ALICE, "discarded");
    branch.rollback_to(checkpoint).unwrap();
    let replacement = add_namespace(&mut branch, ALICE, "replacement");
    assert_eq!(discarded, replacement);
    // Name uniqueness is host registration policy; labels are not identities.
    let duplicate = add_namespace(&mut branch, ALICE, &name);
    assert_ne!(duplicate, ns);
    publish(&mut engine, branch);
    let old = engine.head_snapshot();
    assert_eq!(
        old.descriptor(replacement, Budget::unlimited())
            .unwrap()
            .value
            .name
            .as_str(),
        "replacement"
    );

    let mut branch = begin(&engine, None);
    apply(&mut branch, ALICE, transfer(ns, 0, BOB));
    publish(&mut engine, branch);
    let current = engine.head_snapshot();
    let before = old.descriptor(ns, Budget::unlimited()).unwrap().value;
    let after = current.descriptor(ns, Budget::unlimited()).unwrap().value;
    assert_eq!(before.name, after.name);
    assert_eq!(after.name.as_str(), name);
    assert_eq!((before.owner, after.owner), (ALICE, BOB));
    assert!(current.records_shared_with(&old, ns));
    assert_eq!(
        current.get(ns, b"data", Budget::unlimited()).unwrap().value,
        Some(b"kept".to_vec())
    );
}

#[test]
fn raw_keys_and_prefixes_are_rejected_before_any_lookup_or_write_charge() {
    let mut engine = Engine::new(PriceSchedule::FIXTURE);
    let mut branch = begin(&engine, None);
    let ns = add_namespace(&mut branch, ALICE, "shop");
    let key = vec![b'x'; 20];
    apply(&mut branch, ALICE, put(ns, &key, b""));
    assert_eq!(
        branch.get(ns, &key, Budget::limit(4)).unwrap().value,
        Some(Vec::new())
    );
    assert_eq!(
        branch
            .scan_prefix(ns, &key, Budget::unlimited())
            .unwrap()
            .value
            .len(),
        1
    );
    let spent = branch.spent();
    let receipts = branch.receipts().len();
    let reason = Rejection::InvalidArgument(ArgumentFault::KeyTooLong);
    for length in [21, 8 * 1024 * 1024] {
        let long = vec![b'x'; length];
        for op in [put(ns, &long, b""), delete(ns, &long)] {
            assert_admission(
                branch
                    .estimate(ALICE, op.clone(), Budget::limit(0))
                    .unwrap_err(),
                reason,
            );
            assert_admission(
                branch.execute(ALICE, op, Budget::unlimited()).unwrap_err(),
                reason,
            );
        }
        assert_admission(branch.get(ns, &long, Budget::limit(4)).unwrap_err(), reason);
        assert_admission(
            branch
                .scan_prefix(ns, &long, Budget::unlimited())
                .unwrap_err(),
            reason,
        );
    }
    assert_eq!(branch.spent(), spent);
    assert_eq!(branch.receipts().len(), receipts);
    publish(&mut engine, branch);
    let snapshot = engine.head_snapshot();
    assert_eq!(
        snapshot.get(ns, &key, Budget::limit(4)).unwrap().value,
        Some(Vec::new())
    );
    let long = vec![b'x'; 21];
    assert_admission(
        snapshot.get(ns, &long, Budget::unlimited()).unwrap_err(),
        reason,
    );
    assert_admission(
        snapshot
            .scan_prefix(ns, &long, Budget::unlimited())
            .unwrap_err(),
        reason,
    );
}

#[test]
fn raising_payload_limits_does_not_raise_the_indexed_string_limit() {
    let mut engine = Engine::with_limits(
        PriceSchedule::FIXTURE,
        Limits {
            max_str_len: 8192,
            max_bytes_len: 8192,
            ..Limits::DEFAULT
        },
    );
    let mut branch = begin(&engine, None);
    let ns = add_namespace(&mut branch, ALICE, "shop");
    let name = CellName::new(&"a".repeat(20)).unwrap();
    let payload = CellName::new("payload").unwrap();
    // 64 characters, 128 UTF-8 bytes: exactly the indexed limit.
    let at_cap = "é".repeat(Limits::DEFAULT_INDEXED_LEN / 2);
    let value = Value::str(&at_cap);
    let large = Value::bytes(&vec![0; 8192]);
    let key = RecordKey::new(b"row").unwrap();
    let cells = vec![
        (name.clone(), attr(value.clone())),
        (payload.clone(), field(large.clone())),
    ];
    apply(&mut branch, ALICE, create_record(ns, b"row", cells));
    let spent = branch.spent();
    let receipts = branch.receipts().len();
    let too_long = Value::str(&format!("{at_cap}x"));
    let oversized = vec![(name.clone(), attr(too_long.clone()))];
    for op in [
        create_record(ns, b"bad", oversized.clone()),
        patch_record(ns, b"row", oversized, vec![]),
    ] {
        let reason = Rejection::InvalidArgument(ArgumentFault::ValueTooLarge);
        assert_admission(
            branch
                .estimate(ALICE, op.clone(), Budget::unlimited())
                .unwrap_err(),
            reason,
        );
        assert_admission(
            branch
                .execute(ALICE, op.clone(), Budget::unlimited())
                .unwrap_err(),
            reason,
        );
        assert_admission(
            branch.execute_host(op, Budget::unlimited()).unwrap_err(),
            reason,
        );
    }
    assert_eq!(branch.spent(), spent);
    assert_eq!(branch.receipts().len(), receipts);
    assert_eq!(branch.index_consistent(ns), Some(true));

    let invalid_queries = [
        Query::term(Term::eq(name.clone(), too_long.clone())),
        Query::term(Term::not_eq(name.clone(), too_long.clone())),
        Query::term(Term::starts_with(
            name.clone(),
            &"x".repeat(Limits::DEFAULT_INDEXED_LEN + 1),
        )),
    ];
    let reason = Rejection::InvalidQuery(QueryFault::ValueTooLarge);
    for query in &invalid_queries {
        assert_admission(
            branch
                .query(ns, query, &Projection::ALL, 1, None, Budget::unlimited())
                .unwrap_err(),
            reason,
        );
        assert_admission(
            branch.count(ns, query, Budget::unlimited()).unwrap_err(),
            reason,
        );
    }
    for lower in [
        Bound::Included(too_long.clone()),
        Bound::Excluded(too_long.clone()),
    ] {
        assert_admission(
            branch
                .terms(ns, &name, TypeTag::Str, lower, 1, Budget::unlimited())
                .unwrap_err(),
            reason,
        );
    }
    publish(&mut engine, branch);
    let snapshot = engine.head_snapshot();
    for query in &invalid_queries {
        assert_admission(
            snapshot
                .query(ns, query, &Projection::ALL, 1, None, Budget::unlimited())
                .unwrap_err(),
            reason,
        );
        assert_admission(
            snapshot.count(ns, query, Budget::unlimited()).unwrap_err(),
            reason,
        );
    }
    assert_admission(
        snapshot
            .terms(
                ns,
                &name,
                TypeTag::Str,
                Bound::Included(too_long),
                1,
                Budget::unlimited(),
            )
            .unwrap_err(),
        reason,
    );
    let row = snapshot
        .get_record(ns, &key, &Projection::ALL, Budget::unlimited())
        .unwrap()
        .value
        .unwrap();
    assert!(row.cells.contains(&(payload, field(large))));
    for term in [
        Term::eq(name.clone(), value.clone()),
        Term::starts_with(name.clone(), &at_cap),
    ] {
        assert_eq!(
            snapshot
                .count(ns, &Query::term(term), Budget::unlimited())
                .unwrap()
                .value,
            1
        );
    }
    assert_eq!(
        snapshot
            .terms(
                ns,
                &name,
                TypeTag::Str,
                Bound::Included(value.clone()),
                1,
                Budget::unlimited()
            )
            .unwrap()
            .value,
        vec![value]
    );
}
