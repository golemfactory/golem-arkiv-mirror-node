//! Equality and range proofs: requests, fixtures and the checks every proof
//! test repeats.

use std::{cmp::Ordering, ops::Bound};

use arkiv_db::{
    B256, Budget, Decoder, Encoder, Engine, Limits, PriceSchedule, RecordId, Row, Snapshot, Value,
    proof::{
        EqRequest, ProofLimits, RangeProofLimits, RangeRequest, SnapshotRef, VerifiedEq,
        VerifiedPage, VerifiedRange, VerifiedRangeRows,
    },
};

use super::vocab::{
    Cells, OWNER, add_namespace, apply, begin, create_record, key, ns, publish, rid,
};

/// An engine whose only namespace holds one record per entry of `cells`,
/// keyed by its big-endian position, and those records as rows. The cells of
/// an entry are given in name order, as a row reports them.
pub fn engine_with_rows(
    limits: Limits,
    namespace: &str,
    cells: impl IntoIterator<Item = Cells>,
) -> (Engine, Vec<Row>) {
    let mut engine = Engine::with_limits(PriceSchedule::FIXTURE, limits);
    let mut branch = begin(&engine, None);
    add_namespace(&mut branch, OWNER, namespace);
    let rows = (0u64..)
        .zip(cells)
        .map(|(position, cells)| {
            let key = key(position.to_be_bytes());
            apply(
                &mut branch,
                OWNER,
                create_record(ns(1), key.as_bytes(), cells.clone()),
            );
            Row {
                id: rid(position + 1),
                key: Some(key),
                cells,
            }
        })
        .collect();
    publish(&mut engine, branch);
    (engine, rows)
}

/// A reference to `snapshot` under made-up chain coordinates.
pub fn snapshot_ref(snapshot: &Snapshot) -> SnapshotRef {
    SnapshotRef {
        genesis_hash: B256::repeat_byte(0x11),
        height: snapshot.id().sequence(),
        block_hash: B256::repeat_byte(0x22),
        state_root: snapshot.state_root(),
    }
}

/// The proof `snapshot` produces under default limits.
pub fn eq_proof(snapshot: &Snapshot, request: &EqRequest) -> Vec<u8> {
    snapshot
        .prove_eq(request, ProofLimits::default(), Budget::unlimited())
        .expect("the request is provable")
        .bytes
}

/// The proof `snapshot` produces under default limits.
pub fn range_proof(snapshot: &Snapshot, request: &RangeRequest) -> Vec<u8> {
    snapshot
        .prove_range(request, RangeProofLimits::default(), Budget::unlimited())
        .expect("the request is provable")
        .bytes
}

/// The page of an equality proof about an existing namespace.
pub fn page(verified: VerifiedEq) -> VerifiedPage {
    match verified {
        VerifiedEq::Page(page) => page,
        VerifiedEq::NamespaceAbsent => panic!("the namespace exists"),
    }
}

/// The result of a range proof about an existing namespace.
pub fn complete(verified: VerifiedRange) -> VerifiedRangeRows {
    match verified {
        VerifiedRange::Complete(rows) => rows,
        VerifiedRange::NamespaceAbsent => panic!("the namespace exists"),
    }
}

/// Whether `value` lies within the bounds in the order of its own type.
pub fn in_bounds(value: &Value, lower: &Bound<Value>, upper: &Bound<Value>) -> bool {
    let above = match lower {
        Bound::Unbounded => true,
        Bound::Included(bound) => matches!(
            value.cmp_ordered(bound),
            Some(Ordering::Equal | Ordering::Greater)
        ),
        Bound::Excluded(bound) => value.cmp_ordered(bound) == Some(Ordering::Greater),
    };
    let below = match upper {
        Bound::Unbounded => true,
        Bound::Included(bound) => matches!(
            value.cmp_ordered(bound),
            Some(Ordering::Equal | Ordering::Less)
        ),
        Bound::Excluded(bound) => value.cmp_ordered(bound) == Some(Ordering::Less),
    };
    above && below
}

/// A record id with the proof paths of its row and of its key binding.
pub type RowWitness = (RecordId, Vec<Vec<u8>>, Vec<Vec<u8>>);

/// Reads the counted row witnesses both proof formats end their rows with.
pub fn read_row_witnesses(decoder: &mut Decoder<'_>) -> Vec<RowWitness> {
    let count = decoder.length().unwrap();
    (0..count)
        .map(|_| {
            (
                decoder.get().unwrap(),
                decoder.get().unwrap(),
                decoder.get().unwrap(),
            )
        })
        .collect()
}

pub fn put_row_witnesses(encoder: &mut Encoder, rows: &[RowWitness]) {
    encoder.length(rows.len());
    for (id, row, binding) in rows {
        encoder.put(id);
        encoder.put(row);
        encoder.put(binding);
    }
}

/// `bytes` with the bits of `mask` flipped in the byte at `index`.
pub fn flipped(bytes: &[u8], index: usize, mask: u8) -> Vec<u8> {
    let mut changed = bytes.to_vec();
    changed[index] ^= mask;
    changed
}

/// Asserts that `accepts` refuses every strict prefix of `bytes` and
/// `bytes` followed by one more byte.
pub fn assert_no_other_length_is_accepted(bytes: &[u8], accepts: impl Fn(&[u8]) -> bool) {
    for end in 0..bytes.len() {
        assert!(!accepts(&bytes[..end]), "accepted prefix {end}");
    }
    assert!(
        !accepts(&[bytes, &[0]].concat()),
        "accepted a trailing byte"
    );
}
