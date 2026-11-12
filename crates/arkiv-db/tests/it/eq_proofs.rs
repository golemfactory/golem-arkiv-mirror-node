//! Complete Eq proofs against independent row and Patricia-trie oracles.
//! No expected answer is obtained from Engine::query or the proof producer.
use crate::trie_support::{hash_builder_root, reference};
use arkiv_db::{
    B256, Branch, Budget, Decode, Decoder, Effect, Encoder, Engine, Executed, Limits,
    OperationClass as Op, PriceSchedule, RecordId, Row, Snapshot, StateHeader, Value, decode_exact,
    encode,
    proof::{
        EqCursor, EqQuery, EqRequest, ProofError, ProofLimits, SnapshotRef, VerifiedContinuation,
        VerifiedEq, verify_eq,
    },
};
use std::collections::BTreeMap;

use crate::support::{
    object_store::reopen,
    proofs::{
        RowWitness, assert_no_other_length_is_accepted, eq_proof, flipped, page, put_row_witnesses,
        read_row_witnesses, snapshot_ref,
    },
    vocab::{
        Cells, OWNER, add_namespace, apply, attr, begin, charged, create_record, delete_record,
        field, key, name, ns, patch_record, publish, rid,
    },
};

fn request(snapshot: &Snapshot, value: Value, limit: u32) -> EqRequest {
    EqRequest {
        snapshot: snapshot_ref(snapshot),
        query: EqQuery {
            namespace: ns(1),
            attribute: name("kind"),
            value,
        },
        limit,
        continuation: None,
    }
}
fn verify(
    request: &EqRequest,
    previous: Option<&VerifiedContinuation>,
    bytes: &[u8],
) -> Result<VerifiedEq, ProofError> {
    let result = verify_eq(
        request,
        previous,
        bytes,
        ProofLimits::default(),
        Limits::DEFAULT,
    );
    // Run every honest and adversarial test through the inspecting entrypoint
    // as well: partial point traces must never escape a failed Eq witness.
    let inspected = arkiv_db::proof::verify_eq_inspected(
        request,
        previous,
        bytes,
        ProofLimits::default(),
        Limits::DEFAULT,
    );
    assert_eq!(result, inspected.map(|(result, _)| result));
    result
}
fn cells(value: Value) -> Cells {
    vec![
        (name("kind"), attr(value)),
        (name("secret"), field(Value::bytes(b"all cells included"))),
    ]
}
/// Creates a record keyed by the big-endian `ordinal`.
fn create(branch: &mut Branch, namespace: u64, ordinal: u64, cells: Cells) -> Executed {
    let op = create_record(ns(namespace), ordinal.to_be_bytes(), cells);
    apply(branch, OWNER, op)
}
fn fixture(count: u64) -> (Engine, Vec<Row>) {
    let mut engine = Engine::new(PriceSchedule::FIXTURE);
    let mut branch = begin(&engine, None);
    for _ in 0..2 {
        add_namespace(&mut branch, OWNER, "proof");
    }
    let mut model = Vec::new();
    for ordinal in 1..=count {
        let value = if ordinal % 4 == 0 {
            Value::U64(7)
        } else {
            Value::str("book")
        };
        let cells = cells(value);
        let effect = create(&mut branch, 1, ordinal, cells.clone()).effect;
        assert_eq!(effect, Effect::RecordCreated { id: rid(ordinal) });
        model.push(Row {
            id: rid(ordinal),
            key: Some(key(ordinal.to_be_bytes())),
            cells,
        });
    }
    publish(&mut engine, branch);
    (engine, model)
}
fn expected(model: &[Row], query: &EqQuery) -> Vec<Row> {
    model
        .iter()
        .filter(|row| {
            row.cells.iter().any(|(name, cell)| {
                name == &query.attribute && cell.is_attribute() && cell.value == query.value
            })
        })
        .cloned()
        .collect()
}
fn paged_rows(snapshot: &Snapshot, mut request: EqRequest, sizes: &[u32]) -> Vec<Row> {
    let mut all = Vec::new();
    let mut previous = None;
    let mut index = 0;
    loop {
        request.limit = sizes[index % sizes.len()];
        let bytes = eq_proof(snapshot, &request);
        let page = page(verify(&request, previous.as_ref(), &bytes).unwrap());
        all.extend(page.rows);
        previous = page.next;
        request.continuation = previous.as_ref().map(|p| p.cursor().clone());
        index += 1;
        if previous.is_none() {
            return all;
        }
    }
}

#[derive(Clone)]
struct Wire {
    snapshot: SnapshotRef,
    digest: B256,
    tag: u8,
    header: StateHeader,
    catalog: Vec<Vec<u8>>,
    term: Vec<Vec<u8>>,
    ids: Vec<RecordId>,
    rows: Vec<RowWitness>,
    next: Option<EqCursor>,
}
impl Wire {
    fn parse(bytes: &[u8]) -> Self {
        let mut d = Decoder::new(bytes);
        assert_eq!(d.u32().unwrap(), 2);
        let snapshot = d.get().unwrap();
        let digest = d.get().unwrap();
        let tag = d.u8().unwrap();
        let header = d.get().unwrap();
        let catalog = d.get().unwrap();
        let (term, ids, rows, next) = if tag == 2 {
            let term = d.get().unwrap();
            let ids = d.get().unwrap();
            let rows = read_row_witnesses(&mut d);
            (term, ids, rows, d.get().unwrap())
        } else {
            (vec![], vec![], vec![], None)
        };
        d.finish().unwrap();
        Self {
            snapshot,
            digest,
            tag,
            header,
            catalog,
            term,
            ids,
            rows,
            next,
        }
    }
    fn bytes(&self) -> Vec<u8> {
        let mut e = Encoder::new();
        e.u32(2);
        e.put(&self.snapshot);
        e.put(&self.digest);
        e.u8(self.tag);
        e.put(&self.header);
        e.put(&self.catalog);
        if self.tag == 2 {
            e.put(&self.term);
            e.put(&self.ids);
            put_row_witnesses(&mut e, &self.rows);
            e.put(&self.next);
        }
        e.into_bytes()
    }
}

#[test]
fn pages_match_independent_scan_and_posting_root_with_changing_sizes() {
    let (engine, model) = fixture(93);
    let snapshot = engine.head_snapshot();
    for value in [
        Value::str("book"),
        Value::U64(7),
        Value::Bool(false),
        Value::I64(7),
        Value::str(""),
    ] {
        let request = request(&snapshot, value, 3);
        let expected = expected(&model, &request.query);
        for sizes in [&[1][..], &[3][..], &[64][..], &[1, 3, 64][..]] {
            assert_eq!(paged_rows(&snapshot, request.clone(), sizes), expected);
        }
        let wire = Wire::parse(&eq_proof(&snapshot, &request));
        let wanted: Vec<_> = expected.iter().map(|row| row.id).collect();
        assert_eq!(wire.ids, wanted);
        if !wanted.is_empty() {
            let leaves = wanted
                .iter()
                .map(|id| (id.ordinal().to_be_bytes().to_vec(), vec![1]))
                .collect();
            let posting_root = hash_builder_root(&leaves);
            assert_eq!(posting_root, reference::root(&leaves));
            // An independent term key, no layout::TermKey reuse: the name and
            // its 00 terminator, the tag, presence 01, then the value (a
            // `Str` shifted by one and terminated, a scalar as is).
            let mut term_key = b"kind\0".to_vec();
            let (tag, value) = match &request.query.value {
                Value::Str(s) => (2, s.bytes().map(|b| b + 1).chain([0]).collect::<Vec<u8>>()),
                Value::U64(n) => (13, n.to_be_bytes().to_vec()),
                _ => unreachable!(),
            };
            term_key.extend([tag, 1]);
            term_key.extend(&value);
            let roots = snapshot.try_namespace_roots(ns(1)).unwrap().unwrap();
            let leaf = arkiv_db_trie::walk(roots.terms.0, &term_key, &wire.term)
                .unwrap()
                .unwrap();
            assert_eq!(&leaf[..32], posting_root.as_slice());
            assert_eq!(&leaf[32..], &(wanted.len() as u64).to_be_bytes());
        }
    }
}

#[test]
fn namespace_absence_term_absence_and_genesis_are_distinct() {
    let engine = Engine::new(PriceSchedule::FIXTURE);
    let genesis = engine.head_snapshot();
    let req = request(&genesis, Value::Bool(true), 1);
    assert_eq!(
        verify(&req, None, &eq_proof(&genesis, &req)),
        Ok(VerifiedEq::NamespaceAbsent)
    );
    let (engine, _) = fixture(0);
    let snapshot = engine.head_snapshot();
    let mut req = request(&snapshot, Value::Bool(true), 1);
    let page = page(verify(&req, None, &eq_proof(&snapshot, &req)).unwrap());
    assert!(page.rows.is_empty());
    assert!(page.next.is_none());
    req.query.namespace = ns(999);
    assert_eq!(
        verify(&req, None, &eq_proof(&snapshot, &req)),
        Ok(VerifiedEq::NamespaceAbsent)
    );
}

#[test]
fn forged_cursors_cannot_skip_first_page_or_change_identity() {
    let (engine, _) = fixture(9);
    let snapshot = engine.head_snapshot();
    let mut req = request(&snapshot, Value::str("book"), 1);
    let page = page(verify(&req, None, &eq_proof(&snapshot, &req)).unwrap());
    let previous = page.next.unwrap();
    req.continuation = Some(previous.cursor().clone());
    let bytes = eq_proof(&snapshot, &req);
    assert_eq!(verify(&req, None, &bytes), Err(ProofError::CursorMismatch));
    assert!(verify(&req, Some(&previous), &bytes).is_ok());
    for mutation in 0..6 {
        let mut bad = req.clone();
        match mutation {
            0 => bad.continuation.as_mut().unwrap().last_id = rid(3),
            1 => bad.query.namespace = ns(2),
            2 => bad.query.value = Value::U64(7),
            3 => bad.snapshot.height += 1,
            4 => bad.snapshot.block_hash = B256::ZERO,
            _ => bad.snapshot.state_root = B256::ZERO,
        }
        assert_eq!(
            verify(&bad, Some(&previous), &bytes),
            Err(ProofError::CursorMismatch)
        );
    }
    req.continuation = None;
    assert_eq!(
        verify(&req, Some(&previous), &bytes),
        Err(ProofError::CursorMismatch)
    );
}

#[test]
fn fixed_root_rejects_omissions_membership_only_pages_and_all_witness_mutations() {
    let (engine, _) = fixture(9);
    let snapshot = engine.head_snapshot();
    let req = request(&snapshot, Value::str("book"), 3);
    let original = Wire::parse(&eq_proof(&snapshot, &req));
    type Mutation = (&'static str, fn(&mut Wire));
    let mutations: [Mutation; 20] = [
        // Valid row membership does not prove completeness.
        ("last posting id dropped", |wire| {
            wire.ids.pop();
        }),
        ("posting ids swapped", |wire| wire.ids.swap(0, 1)),
        ("posting id repeated", |wire| wire.ids[1] = wire.ids[0]),
        ("non-member in place of a posting id", |wire| {
            wire.ids[0] = rid(4)
        }),
        ("row witness dropped", |wire| {
            wire.rows.remove(1);
        }),
        ("row witnesses swapped", |wire| wire.rows.swap(0, 1)),
        ("row witness repeated", |wire| {
            wire.rows[1] = wire.rows[0].clone()
        }),
        ("row witness under another id", |wire| {
            wire.rows[0].0 = rid(4)
        }),
        ("row path emptied", |wire| wire.rows[0].1.clear()),
        ("key binding path emptied", |wire| wire.rows[0].2.clear()),
        ("term path emptied", |wire| wire.term.clear()),
        ("catalog path emptied", |wire| wire.catalog.clear()),
        ("state header changed", |wire| {
            wire.header.next_namespace += 1
        }),
        ("false terminal claim", |wire| wire.next = None),
        ("continuation after another id", |wire| {
            wire.next.as_mut().unwrap().last_id = rid(7)
        }),
        ("row path node repeated", |wire| {
            let node = wire.rows[0].1[0].clone();
            wire.rows[0].1.push(node);
        }),
        ("row path node bit flipped", |wire| {
            wire.rows[0].1[0][0] ^= 1
        }),
        ("key binding node bit flipped", |wire| {
            wire.rows[0].2[0][0] ^= 1
        }),
        ("query digest zeroed", |wire| wire.digest = B256::ZERO),
        ("tag of an absent namespace", |wire| wire.tag = 1),
    ];
    for (what, mutate) in mutations {
        let mut wire = original.clone();
        mutate(&mut wire);
        assert!(
            verify(&req, None, &wire.bytes()).is_err(),
            "accepted: {what}"
        );
    }
    let bytes = original.bytes();
    assert_no_other_length_is_accepted(&bytes, |bytes| verify(&req, None, bytes).is_ok());
    let mut extra_node = original;
    extra_node.catalog.push(vec![0x80]);
    assert!(verify(&req, None, &extra_node.bytes()).is_err());
    for byte in [0, 3, 7, 50, bytes.len() - 1] {
        assert!(verify(&req, None, &flipped(&bytes, byte, 0xff)).is_err());
    }
}

#[test]
fn history_retains_pages_across_patch_delete_recreate_rollback_and_new_namespaces() {
    let (mut engine, mut model) = fixture(18);
    let old = engine.head_snapshot();
    let old_model = model.clone();
    let old_req = request(&old, Value::str("book"), 3);
    let old_bytes = eq_proof(&old, &old_req);
    let mut branch = begin(&engine, None);
    let mark = branch.checkpoint();
    apply(&mut branch, OWNER, delete_record(ns(1), 1u64.to_be_bytes()));
    branch.rollback_to(mark).unwrap();
    let set = vec![(name("kind"), field(Value::str("book")))];
    let patch = patch_record(ns(1), 2u64.to_be_bytes(), set, vec![]);
    apply(&mut branch, OWNER, patch);
    model[1].cells[0].1 = field(Value::str("book"));
    apply(&mut branch, OWNER, delete_record(ns(1), 3u64.to_be_bytes()));
    model.retain(|row| row.id != rid(3));
    create(&mut branch, 1, 3, cells(Value::str("book")));
    model.push(Row {
        id: rid(19),
        key: Some(key(3u64.to_be_bytes())),
        cells: cells(Value::str("book")),
    });
    for i in 0..100 {
        create(&mut branch, 2, i, cells(Value::Bool(true)));
    }
    publish(&mut engine, branch);
    let latest = engine.head_snapshot();
    assert_eq!(eq_proof(&old, &old_req), old_bytes);
    assert_eq!(
        paged_rows(&old, old_req.clone(), &[1, 3, 64]),
        expected(&old_model, &old_req.query)
    );
    let req = request(&latest, Value::str("book"), 3);
    assert_eq!(
        paged_rows(&latest, req.clone(), &[3]),
        expected(&model, &req.query)
    );
}

#[test]
fn resource_caps_reject_before_large_allocation_and_never_return_partial_rows() {
    let (engine, _) = fixture(9);
    let snapshot = engine.head_snapshot();
    let req = request(&snapshot, Value::str("book"), 3);
    let bytes = eq_proof(&snapshot, &req);
    for caps in [
        ProofLimits {
            max_proof_bytes: bytes.len() - 1,
            ..ProofLimits::default()
        },
        ProofLimits {
            max_posting_ids: 6,
            ..ProofLimits::default()
        },
        ProofLimits {
            max_nodes: 0,
            ..ProofLimits::default()
        },
        ProofLimits {
            max_allocated_bytes: 8,
            ..ProofLimits::default()
        },
    ] {
        assert_eq!(
            verify_eq(&req, None, &bytes, caps, Limits::DEFAULT),
            Err(ProofError::ProofLimitExceeded)
        );
    }
    assert_eq!(
        verify_eq(
            &req,
            None,
            &bytes,
            ProofLimits {
                max_verification_work: 1,
                ..ProofLimits::default()
            },
            Limits::DEFAULT
        ),
        Err(ProofError::BudgetExceeded)
    );
    assert_eq!(
        snapshot
            .prove_eq(&req, ProofLimits::default(), Budget::limit(1))
            .unwrap_err(),
        ProofError::BudgetExceeded
    );
    assert_eq!(
        snapshot
            .prove_eq(
                &req,
                ProofLimits {
                    max_posting_ids: 6,
                    ..ProofLimits::default()
                },
                Budget::unlimited()
            )
            .unwrap_err(),
        ProofError::ProofLimitExceeded
    );
    for limit in [0, 65, u32::MAX] {
        let mut bad = req.clone();
        bad.limit = limit;
        assert_eq!(verify(&bad, None, &bytes), Err(ProofError::InvalidRequest));
    }
    let mut bad = req.clone();
    bad.query.value = Value::bytes(b"x");
    assert_eq!(
        verify(&bad, None, &bytes),
        Err(ProofError::UnsupportedProof)
    );
    // A huge path count with almost no payload cannot allocate its vector.
    let mut bad = bytes.clone();
    let count_at = 4 + 108 + 32 + 1 + StateHeader::MIN_ENCODED_LEN;
    bad[count_at..count_at + 4].copy_from_slice(&u32::MAX.to_be_bytes());
    assert_eq!(
        verify(&req, None, &bad),
        Err(ProofError::ProofLimitExceeded)
    );
    let mut bad = bytes;
    bad[..4].copy_from_slice(&999u32.to_be_bytes());
    assert_eq!(
        verify(&req, None, &bad),
        Err(ProofError::UnsupportedVersion)
    );
}

#[test]
fn codecs_have_literal_version_two_vectors_and_reject_unknown_tags() {
    let query = EqQuery {
        namespace: ns(1),
        attribute: name("a"),
        value: Value::I64(-2),
    };
    let expected = [
        0, 0, 0, 2, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 1, b'a', 17, 127, 255, 255, 255, 255, 255,
        255, 254, 1, 1,
    ];
    assert_eq!(encode(&query), expected);
    assert_eq!(decode_exact::<EqQuery>(&expected).unwrap(), query);
    // `i32` is type id 16 with four sign-flipped bytes.
    let narrow = EqQuery {
        namespace: ns(1),
        attribute: name("a"),
        value: Value::I32(-2),
    };
    let mut expected_narrow = vec![
        0, 0, 0, 2, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 1, b'a', 16, 127, 255, 255, 254,
    ];
    // Projection flags bind both the structural key and all committed cells.
    expected_narrow.extend([1, 1]);
    assert_eq!(encode(&narrow), expected_narrow);
    assert_eq!(decode_exact::<EqQuery>(&expected_narrow).unwrap(), narrow);
    // A layout-1 (or any other) proof version is rejected.
    expected_narrow[3] = 1;
    assert!(decode_exact::<EqQuery>(&expected_narrow).is_err());
    let snapshot = SnapshotRef {
        genesis_hash: B256::repeat_byte(0x11),
        height: 9,
        block_hash: B256::repeat_byte(0x22),
        state_root: B256::repeat_byte(0x33),
    };
    let mut expected_snapshot = vec![0, 0, 0, 2];
    expected_snapshot.extend([0x11; 32]);
    expected_snapshot.extend([0, 0, 0, 0, 0, 0, 0, 9]);
    expected_snapshot.extend([0x22; 32]);
    expected_snapshot.extend([0x33; 32]);
    assert_eq!(encode(&snapshot), expected_snapshot);
    let cursor = EqCursor {
        snapshot,
        query_digest: B256::repeat_byte(0x44),
        last_id: rid(7),
    };
    let mut expected_cursor = vec![0, 0, 0, 2];
    expected_cursor.extend(expected_snapshot);
    expected_cursor.extend([0x44; 32]);
    expected_cursor.extend([0, 0, 0, 0, 0, 0, 0, 7]);
    assert_eq!(encode(&cursor), expected_cursor);
    assert_eq!(decode_exact::<EqCursor>(&expected_cursor).unwrap(), cursor);
    for at in [3, 17, 26, 27] {
        let mut bytes = expected;
        bytes[at] = 99;
        assert!(decode_exact::<EqQuery>(&bytes).is_err());
    }
    let mut extra = expected_cursor;
    extra.push(0);
    assert!(decode_exact::<EqCursor>(&extra).is_err());
}

#[test]
fn literal_charge_vectors_cover_full_posting_and_all_output_bytes() {
    // Empty-catalog proof is 497 bytes: envelope 145 + state header 348 +
    // catalog path count 4. No trie node exists.
    let engine = Engine::new(PriceSchedule::FIXTURE);
    let snapshot = engine.head_snapshot();
    let req = request(&snapshot, Value::Bool(true), 1);
    let result = snapshot
        .prove_eq(&req, ProofLimits::default(), Budget::unlimited().traced())
        .unwrap();
    assert_eq!(result.bytes.len(), 497);
    assert_eq!(result.receipt.model_version, 4);
    assert_eq!(result.receipt.spent.units(), 498);
    let ledger = result.receipt.ledger.unwrap();
    assert_eq!(
        ledger
            .entries
            .iter()
            .map(|e| (e.class, e.count))
            .collect::<Vec<_>>(),
        vec![(Op::ProofByte, 497), (Op::DirectoryLookup, 1)]
    );
    let (engine, _) = fixture(9);
    let snapshot = engine.head_snapshot();
    let req = request(&snapshot, Value::str("book"), 1);
    let result = snapshot
        .prove_eq(&req, ProofLimits::default(), Budget::unlimited().traced())
        .unwrap();
    let ledger = result.receipt.ledger.unwrap();
    assert_eq!(charged(&ledger, Op::PostingEntry), 7); // full set, even for one row
    assert_eq!(charged(&ledger, Op::RecordProbe), 1);
    assert_eq!(charged(&ledger, Op::ResultRow), 1);
    assert_eq!(charged(&ledger, Op::ReadByte), 40); // key 8 + kind 4 + book 4 + secret 6 + payload 18
    assert_eq!(charged(&ledger, Op::ProofByte), result.bytes.len() as u64);
    assert_eq!(charged(&ledger, Op::TermVisit), 0);
    assert!(charged(&ledger, Op::ProofNode) >= result.supplied_nodes as u64);
}

/// Independently encode all four map levels. `mutation` commits an
/// inconsistent state under a new trusted root, to exercise semantic checks
/// beyond detecting a changed node hash. This fixture has no Engine.
fn independent_witness(mutation: u8) -> (EqRequest, Wire) {
    let mut rows = BTreeMap::new();
    let mut keys = BTreeMap::new();
    let mut posting = BTreeMap::new();
    for n in 1..=2u64 {
        let raw_key = [b'k', n as u8];
        let mut row = vec![
            2, raw_key[0], raw_key[1], 0, 0, 0, 1, 0, 0, 0, 1, b'a', 1, 1, 1,
        ];
        if n == 1 {
            match mutation {
                1 => row[12] = 2, // field
                2 => row[14] = 0, // value false
                3 => {
                    row[13] = 3;
                    row.pop();
                    row.extend(1u64.to_be_bytes());
                } // U64
                4 => row[2] = 9,  // row's key lacks binding
                5 => row[7..11].copy_from_slice(&u32::MAX.to_be_bytes()), // oversized name
                6 => row[3..7].copy_from_slice(&65u32.to_be_bytes()), // too many cells
                7 => row.push(0), // leftover stored-row bytes
                _ => {}
            }
        }
        rows.insert(n.to_be_bytes().to_vec(), row);
        let mut key = vec![2];
        key.extend_from_slice(&raw_key);
        let bound = if mutation == 8 && n == 1 { 2u64 } else { n };
        keys.insert(key, bound.to_be_bytes().to_vec());
        posting.insert(n.to_be_bytes().to_vec(), vec![1]);
    }
    let posting_root = reference::root(&posting);
    assert_eq!(posting_root, hash_builder_root(&posting));
    // `a = true`: name, terminator, Bool (1), presence 01, value 01.
    let term_key = vec![b'a', 0, 1, 1, 1];
    let mut posting_leaf = posting_root.to_vec();
    let count = match mutation {
        10 => 3u64,
        11 => 4097,
        _ => 2,
    };
    posting_leaf.extend(count.to_be_bytes());
    if mutation == 12 {
        posting_leaf[0] ^= 1;
    }
    let term_map = BTreeMap::from([(term_key.clone(), posting_leaf)]);
    // Descriptor: id 1, name n, owner [7;20], revision 0, allocator 3.
    let mut catalog_leaf = (if mutation == 9 { 2u64 } else { 1 })
        .to_be_bytes()
        .to_vec();
    catalog_leaf.extend([0, 0, 0, 1, b'n']);
    catalog_leaf.extend([7; 20]);
    catalog_leaf.extend([0; 8]);
    catalog_leaf.extend(3u64.to_be_bytes());
    for (root, count) in [
        (alloy_trie::EMPTY_ROOT_HASH, 0u64),
        (reference::root(&keys), 2),
        (reference::root(&rows), 2),
        (reference::root(&term_map), 1),
    ] {
        catalog_leaf.extend(root.as_slice());
        catalog_leaf.extend(count.to_be_bytes());
    }
    let catalog = BTreeMap::from([(1u64.to_be_bytes().to_vec(), catalog_leaf)]);
    let header = StateHeader {
        format: 2,
        catalog_root: reference::root(&catalog),
        catalog_len: 1,
        next_namespace: 2,
        pricing: PriceSchedule::FIXTURE,
        host_root: alloy_trie::EMPTY_ROOT_HASH,
        host_len: 0,
    };
    let query = EqQuery {
        namespace: ns(1),
        attribute: name("a"),
        value: Value::Bool(true),
    };
    let snapshot = SnapshotRef {
        genesis_hash: B256::repeat_byte(0x11),
        height: 9,
        block_hash: B256::repeat_byte(0x22),
        state_root: header.root(),
    };
    let req = EqRequest {
        snapshot: snapshot.clone(),
        query: query.clone(),
        limit: 2,
        continuation: None,
    };
    let witnesses = (1..=2u64)
        .map(|n| {
            let mut key = vec![0; 33];
            key[0] = 2;
            key[1] = b'k';
            key[2] = n as u8;
            (
                rid(n),
                reference::proof(&rows, &n.to_be_bytes()),
                reference::proof(&keys, &key),
            )
        })
        .collect();
    let wire = Wire {
        snapshot,
        digest: query.digest(),
        tag: 2,
        header,
        catalog: reference::proof(&catalog, &1u64.to_be_bytes()),
        term: reference::proof(&term_map, &term_key),
        ids: vec![rid(1), rid(2)],
        rows: witnesses,
        next: None,
    };
    (req, wire)
}

#[test]
fn independent_trie_fixture_verifies_without_engine_and_rejects_inconsistent_authenticated_rows() {
    let (req, wire) = independent_witness(0);
    let page = page(verify(&req, None, &wire.bytes()).unwrap());
    assert_eq!(page.rows.len(), 2);
    assert_eq!(
        page.rows[0],
        Row {
            id: rid(1),
            key: Some(key([b'k', 1])),
            cells: vec![(name("a"), attr(Value::Bool(true)))]
        }
    );
    for mutation in 1..=12 {
        let (req, wire) = independent_witness(mutation);
        assert!(
            verify(&req, None, &wire.bytes()).is_err(),
            "accepted independent inconsistent state {mutation}"
        );
    }
}

#[test]
fn unavailable_storage_is_an_error_and_memory_and_stored_proof_bytes_match() {
    let (engine, _) = fixture(9);
    let original = engine.head_snapshot();
    let (store, reopened) = reopen(&original.state_header(), original.staged_objects());
    let snapshot = reopened.head_snapshot();
    let req = request(&original, Value::str("book"), 3);
    assert_eq!(eq_proof(&snapshot, &req), eq_proof(&original, &req));
    for object in original.staged_objects() {
        store.override_object(object.space, object.id, None);
    }
    assert_eq!(
        snapshot
            .prove_eq(&req, ProofLimits::default(), Budget::unlimited())
            .unwrap_err(),
        ProofError::StorageUnavailable
    );
}

#[test]
fn unrelated_namespace_growth_does_not_add_posting_or_row_work() {
    let (mut engine, _) = fixture(9);
    let first = engine.head_snapshot();
    let req = request(&first, Value::str("book"), 3);
    let before = first
        .prove_eq(&req, ProofLimits::default(), Budget::unlimited().traced())
        .unwrap();
    let mut branch = begin(&engine, None);
    for n in 0..300 {
        create(&mut branch, 2, n, cells(Value::str("book")));
    }
    publish(&mut engine, branch);
    let after_snapshot = engine.head_snapshot();
    let req = request(&after_snapshot, Value::str("book"), 3);
    let after = after_snapshot
        .prove_eq(&req, ProofLimits::default(), Budget::unlimited().traced())
        .unwrap();
    assert_eq!(before.receipt, after.receipt);
    assert_eq!(before.bytes.len(), after.bytes.len());
    let before = Wire::parse(&before.bytes);
    let after = Wire::parse(&after.bytes);
    assert_eq!(before.term, after.term);
    assert_eq!(before.ids, after.ids);
    assert_eq!(before.rows, after.rows);
}

#[test]
fn largest_rows_are_bounded_before_materialization_and_respect_trusted_genesis_limits() {
    let mut engine = Engine::new(PriceSchedule::FIXTURE);
    let mut branch = begin(&engine, None);
    add_namespace(&mut branch, OWNER, "large");
    let mut fields = vec![(name("kind"), attr(Value::Bool(true)))];
    for i in 0..63 {
        fields.push((
            name(&format!("f{i:02}")),
            field(Value::bytes(&vec![i; 4096])),
        ));
    }
    create(&mut branch, 1, 1, fields);
    publish(&mut engine, branch);
    let snapshot = engine.head_snapshot();
    let req = request(&snapshot, Value::Bool(true), 1);
    let bytes = eq_proof(&snapshot, &req);
    let page = page(verify(&req, None, &bytes).unwrap());
    assert_eq!(page.rows[0].cells.len(), 64);
    assert_eq!(page.rows[0].cells[0].1.value, Value::bytes(&vec![0; 4096]));
    for limits in [
        Limits {
            max_cells: 63,
            ..Limits::DEFAULT
        },
        Limits {
            max_str_len: 4095,
            max_bytes_len: 4095,
            ..Limits::DEFAULT
        },
    ] {
        assert!(verify_eq(&req, None, &bytes, ProofLimits::default(), limits).is_err());
    }
    // Meter budget cannot fund a full row, so generation fails before the
    // row lookup/proof serialization and returns no partial evidence.
    assert_eq!(
        snapshot
            .prove_eq(&req, ProofLimits::default(), Budget::limit(200_000))
            .unwrap_err(),
        ProofError::BudgetExceeded
    );
    assert_eq!(
        snapshot
            .prove_eq(
                &req,
                ProofLimits {
                    max_proof_bytes: 200_000,
                    ..ProofLimits::default()
                },
                Budget::unlimited()
            )
            .unwrap_err(),
        ProofError::ProofLimitExceeded
    );
}

#[test]
fn complete_posting_cap_admits_4096_and_rejects_one_more_before_iteration() {
    let mut engine = Engine::new(PriceSchedule::FIXTURE);
    let mut branch = begin(&engine, None);
    add_namespace(&mut branch, OWNER, "limit");
    for n in 1..=4096 {
        create(&mut branch, 1, n, cells(Value::Bool(true)));
    }
    publish(&mut engine, branch);
    let snapshot = engine.head_snapshot();
    let req = request(&snapshot, Value::Bool(true), 1);
    let page = page(verify(&req, None, &eq_proof(&snapshot, &req)).unwrap());
    assert_eq!(page.posting_count, 4096);
    assert_eq!(page.rows[0].id, rid(1));
    assert!(page.next.is_some());
    let mut branch = begin(&engine, None);
    create(&mut branch, 1, 4097, cells(Value::Bool(true)));
    publish(&mut engine, branch);
    let snapshot = engine.head_snapshot();
    let req = request(&snapshot, Value::Bool(true), 1);
    assert_eq!(
        snapshot
            .prove_eq(&req, ProofLimits::default(), Budget::unlimited())
            .unwrap_err(),
        ProofError::ProofLimitExceeded
    );
}

#[test]
fn every_supported_type_and_historical_expiry_use_selected_state_only() {
    let mut engine = Engine::new(PriceSchedule::FIXTURE);
    let mut branch = begin(&engine, None);
    add_namespace(&mut branch, OWNER, "types");
    let values = [
        Value::Bool(false),
        Value::Bool(true),
        Value::I64(i64::MIN),
        Value::I64(-1),
        Value::I64(0),
        Value::I64(i64::MAX),
        Value::U64(0),
        Value::U64(u64::MAX),
        Value::str(""),
        Value::str(&"é".repeat(32)),
    ];
    let mut model = Vec::new();
    for (index, value) in values.iter().enumerate() {
        let ordinal = index as u64 + 1;
        let mut fields = cells(value.clone());
        fields.push((name("$expiresAt"), attr(Value::U64(2))));
        create(&mut branch, 1, ordinal, fields.clone());
        fields.sort_by(|a, b| a.0.cmp(&b.0));
        model.push(Row {
            id: rid(ordinal),
            key: Some(key(ordinal.to_be_bytes())),
            cells: fields,
        });
    }
    publish(&mut engine, branch);
    let before = engine.head_snapshot();
    let mut branch = begin(&engine, None);
    let expired = arkiv_db::expire_records(
        &mut branch,
        ns(1),
        &name("$expiresAt"),
        arkiv_db::BlockNumber(2),
        Budget::unlimited(),
        Budget::unlimited(),
    )
    .unwrap();
    assert_eq!(expired.len(), values.len());
    publish(&mut engine, branch);
    let after = engine.head_snapshot();
    for value in values {
        let req = request(&before, value.clone(), 3);
        assert_eq!(
            paged_rows(&before, req.clone(), &[1, 3, 64]),
            expected(&model, &req.query)
        );
        assert!(paged_rows(&after, request(&after, value, 3), &[1, 3, 64]).is_empty());
    }
}

#[test]
fn empty_namespace_proof_has_hand_built_canonical_bytes() {
    fn independent_digest(domain: &[u8], bytes: &[u8]) -> B256 {
        let mut input = vec![domain.len() as u8];
        input.extend(domain);
        input.extend(bytes);
        alloy_primitives::keccak256(input)
    }
    // Header: format 2, empty catalog, allocator 1, schedule 7 with
    // thirty-one weights of one, empty host map. No production codecs.
    let mut header = vec![0, 0, 0, 2];
    header.extend(alloy_trie::EMPTY_ROOT_HASH.as_slice());
    header.extend([0; 8]);
    header.extend([0, 0, 0, 0, 0, 0, 0, 1]);
    header.extend([0, 0, 0, 7, 0, 0, 0, 31]);
    for _ in 0..31 {
        header.extend([0, 0, 0, 0, 0, 0, 0, 1]);
    }
    header.extend(alloy_trie::EMPTY_ROOT_HASH.as_slice());
    header.extend([0; 8]);
    let root = independent_digest(b"arkiv/state/v1", &header);
    let query_bytes = [
        0, 0, 0, 2, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 1, b'a', 1, 1, 1, 1,
    ];
    let query_digest = independent_digest(b"arkiv/eq-query/v1", &query_bytes);
    let mut expected = vec![0, 0, 0, 2, 0, 0, 0, 2]; // response version, snapshot version
    expected.extend([0x11; 32]);
    expected.extend([0; 8]);
    expected.extend([0x22; 32]);
    expected.extend(root.as_slice());
    expected.extend(query_digest.as_slice());
    expected.push(1);
    expected.extend(header);
    expected.extend([0; 4]);
    let engine = Engine::new(PriceSchedule::uniform(7, 1));
    let snapshot = engine.head_snapshot();
    let mut req = request(&snapshot, Value::Bool(true), 1);
    req.query.attribute = name("a");
    assert_eq!(req.snapshot.state_root, root);
    assert_eq!(req.query.digest(), query_digest);
    assert_eq!(eq_proof(&snapshot, &req), expected);
    assert_eq!(
        verify(&req, None, &expected),
        Ok(VerifiedEq::NamespaceAbsent)
    );
}

#[test]
fn inspection_binds_map_roots_keys_and_complete_posting_reconstruction() {
    use arkiv_db::MapKind;
    use arkiv_db_trie::PathTerminal;
    let (req, wire) = independent_witness(0);
    let (result, inspection) = arkiv_db::proof::verify_eq_inspected(
        &req,
        None,
        &wire.bytes(),
        ProofLimits::default(),
        Limits::DEFAULT,
    )
    .unwrap();
    assert_eq!(inspection.state_header.root(), req.snapshot.state_root);
    assert!(matches!(result, VerifiedEq::Page(_)));
    assert_eq!(
        inspection
            .point_paths
            .iter()
            .map(|p| p.map)
            .collect::<Vec<_>>(),
        vec![
            MapKind::Catalog,
            MapKind::Terms,
            MapKind::Rows,
            MapKind::Keys,
            MapKind::Rows,
            MapKind::Keys
        ]
    );
    assert_eq!(inspection.point_paths[0].root, wire.header.catalog_root);
    assert_eq!(
        inspection.point_paths[0].key,
        req.query.namespace.to_bytes()
    );
    let roots = inspection.namespace.unwrap();
    for path in &inspection.point_paths {
        assert_eq!(path.trace.terminal, PathTerminal::Inclusion);
        assert_eq!(path.trace.nodes[0].hash, path.root);
        match path.map {
            MapKind::Terms => assert_eq!(path.root, roots.terms.0),
            MapKind::Rows => {
                assert_eq!(path.root, roots.rows.0);
                assert_eq!(path.key, path.record_id.unwrap().ordinal().to_be_bytes());
            }
            MapKind::Keys => assert_eq!(path.root, roots.keys.0),
            _ => {}
        }
    }
    let posting = inspection.posting_set.unwrap();
    assert_eq!(posting.ids, vec![rid(1), rid(2)]);
    assert_eq!(posting.selected_ids, posting.ids);
    assert_eq!(posting.authenticated_root, Some(posting.reconstructed_root));
    // Full inspector also honors local resource policy.
    let caps = ProofLimits {
        max_allocated_bytes: 1,
        ..ProofLimits::default()
    };
    assert_eq!(
        arkiv_db::proof::verify_eq_inspected(&req, None, &wire.bytes(), caps, Limits::DEFAULT),
        Err(ProofError::ProofLimitExceeded)
    );
}
