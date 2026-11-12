//! Range predicates on the byte-ordered types: `Bool`,
//! `Str` and the fixed-width byte types (`Addr`, `Bytes4/8/16`, `Bytes32`,
//! `Key`). Their order is golem-db's attribute order: the stored value
//! bytes compared bytewise, so `false < true`, fixed bytes as unsigned
//! big-endian strings, and `Str` by its UTF-8 bytes with a proper prefix
//! first (`"" < "a" < "a\0" < "ab"`).
//!
//! Every result is checked against an independent model that orders by the
//! raw value bytes (never `Value::cmp_ordered` or term bytes): paged query
//! results walked with keyset cursors, `count`, and complete range proofs.

use std::ops::Bound::{self, Excluded, Included, Unbounded};

use arkiv_db::{
    Budget, Engine, Limits, PriceSchedule, Projection, Query, QueryFault, RecordId, Rejection,
    Snapshot, Term, Value,
    proof::{ProofError, RangeProofLimits, RangeQuery, RangeRequest, verify_range},
};

use crate::support::{
    proofs::{complete, range_proof, snapshot_ref},
    vocab::{
        Cells, OWNER, add_namespace, apply, attr, begin, create_record, field, name, ns, publish,
        rid,
    },
    walk::{rows_of, walk},
};

/// The model's sort key: the value's own bytes, independent of the engine's
/// term encoding.
fn model_bytes(value: &Value) -> Vec<u8> {
    match value {
        Value::Bool(flag) => vec![u8::from(*flag)],
        Value::Str(text) => text.as_bytes().to_vec(),
        Value::Addr(bytes) => bytes.to_vec(),
        Value::Bytes4(bytes) => bytes.to_vec(),
        Value::Bytes8(bytes) => bytes.to_vec(),
        Value::Bytes16(bytes) => bytes.to_vec(),
        Value::Bytes32(bytes) | Value::Key(bytes) => bytes.to_vec(),
        other => panic!("not a byte-ordered type: {other:?}"),
    }
}

fn in_range(value: &Value, lower: &Bound<Value>, upper: &Bound<Value>) -> bool {
    let key = model_bytes(value);
    let above = match lower {
        Unbounded => true,
        Included(bound) => key >= model_bytes(bound),
        Excluded(bound) => key > model_bytes(bound),
    };
    let below = match upper {
        Unbounded => true,
        Included(bound) => key <= model_bytes(bound),
        Excluded(bound) => key < model_bytes(bound),
    };
    above && below
}

/// One record per value with attribute `v`, plus distractors that a range
/// on `v` must never return: the same value as a field, the same name with
/// another type, and the value under another name.
struct Fixture {
    engine: Engine,
    /// `(record id, value)` of the attribute records, in id order.
    model: Vec<(RecordId, Value)>,
}

fn fixture(values: &[Value]) -> Fixture {
    let mut engine = Engine::new(PriceSchedule::FIXTURE);
    let mut branch = begin(&engine, None);
    add_namespace(&mut branch, OWNER, "ordered");
    let mut model = Vec::new();
    let mut ordinal = 0u64;
    let mut create = |cells: Cells| {
        ordinal += 1;
        let op = create_record(ns(1), ordinal.to_be_bytes(), cells);
        apply(&mut branch, OWNER, op);
        rid(ordinal)
    };
    for (index, value) in values.iter().enumerate() {
        let id = create(vec![(name("v"), attr(value.clone()))]);
        model.push((id, value.clone()));
        if index % 3 == 0 {
            create(vec![(name("v"), field(value.clone()))]);
            create(vec![(name("w"), attr(value.clone()))]);
            create(vec![(name("v"), attr(Value::U64(7)))]);
        }
    }
    publish(&mut engine, branch);
    Fixture { engine, model }
}

impl Fixture {
    fn snapshot(&self) -> Snapshot {
        self.engine.head_snapshot()
    }

    fn expected(&self, lower: &Bound<Value>, upper: &Bound<Value>) -> Vec<RecordId> {
        self.model
            .iter()
            .filter(|(_, value)| in_range(value, lower, upper))
            .map(|(id, _)| *id)
            .collect()
    }

    /// Walks every page of the range query with keyset cursors.
    fn paged(&self, lower: &Bound<Value>, upper: &Bound<Value>, page: usize) -> Vec<RecordId> {
        let query = Query::term(Term::range(name("v"), lower.clone(), upper.clone()));
        let pages = walk(
            &self.snapshot(),
            ns(1),
            &query,
            &Projection::ID_ONLY,
            Budget::unlimited(),
            |_| page,
            |_| {},
        );
        rows_of(&pages).iter().map(|row| row.id).collect()
    }

    fn count(&self, lower: &Bound<Value>, upper: &Bound<Value>) -> u64 {
        let query = Query::term(Term::range(name("v"), lower.clone(), upper.clone()));
        self.snapshot()
            .count(ns(1), &query, Budget::unlimited())
            .unwrap()
            .value
    }

    fn request(&self, lower: &Bound<Value>, upper: &Bound<Value>) -> RangeRequest {
        RangeRequest {
            snapshot: snapshot_ref(&self.snapshot()),
            query: RangeQuery {
                namespace: ns(1),
                attribute: name("v"),
                lower: lower.clone(),
                upper: upper.clone(),
            },
        }
    }

    /// Generates and verifies a complete range proof; returns its row ids.
    fn proven(&self, lower: &Bound<Value>, upper: &Bound<Value>) -> Vec<RecordId> {
        let request = self.request(lower, upper);
        let bytes = range_proof(&self.snapshot(), &request);
        let verified = verify_range(
            &request,
            &bytes,
            RangeProofLimits::default(),
            Limits::DEFAULT,
        );
        let result = complete(verified.unwrap());
        for row in &result.rows {
            let held = &row
                .cells
                .iter()
                .find(|(cell, _)| cell.as_str() == "v")
                .unwrap()
                .1;
            assert!(held.is_attribute());
            assert!(in_range(&held.value, lower, upper));
        }
        result.rows.iter().map(|row| row.id).collect()
    }

    /// Every bound combination from `bounds`: paged query (pages of 1, 2
    /// and 5), count and range proof all equal the model.
    fn check_all(&self, bounds: &[Value]) {
        let mut sides = vec![Unbounded];
        for bound in bounds {
            sides.push(Included(bound.clone()));
            sides.push(Excluded(bound.clone()));
        }
        for lower in &sides {
            for upper in &sides {
                if *lower == Unbounded && *upper == Unbounded {
                    continue;
                }
                let expected = self.expected(lower, upper);
                for page in [1, 2, 5] {
                    assert_eq!(
                        self.paged(lower, upper, page),
                        expected,
                        "{lower:?}..{upper:?} in pages of {page}"
                    );
                }
                assert_eq!(self.count(lower, upper), expected.len() as u64);
                assert_eq!(self.proven(lower, upper), expected, "{lower:?}..{upper:?}");
            }
        }
    }
}

#[test]
fn bool_ranges_order_false_before_true() {
    let values = [true, false, true, true, false].map(Value::Bool);
    let fixture = fixture(&values);
    fixture.check_all(&[Value::Bool(false), Value::Bool(true)]);
    assert_eq!(
        fixture.paged(&Excluded(Value::Bool(false)), &Unbounded, 2),
        fixture.expected(&Included(Value::Bool(true)), &Unbounded)
    );
}

#[test]
fn str_ranges_order_by_utf8_bytes_with_prefixes_first() {
    let longest_ascii = "z".repeat(arkiv_db::Limits::DEFAULT_INDEXED_LEN);
    // 64 two-byte characters: exactly the 128-byte indexed cap.
    let longest_utf8 = "é".repeat(arkiv_db::Limits::DEFAULT_INDEXED_LEN / 2);
    let texts = [
        "ab",
        "",
        "a\0",
        "a",
        "b",
        "a",
        "Z",
        "é",
        "\u{10FFFF}",
        "ab\0",
        longest_ascii.as_str(),
        longest_utf8.as_str(),
        "abc",
    ];
    let values: Vec<Value> = texts.iter().map(|text| Value::str(text)).collect();
    let fixture = fixture(&values);
    let bounds: Vec<Value> = [
        "",
        "\0",
        "a",
        "a\0",
        "ab",
        "aa",
        "é",
        longest_ascii.as_str(),
        longest_utf8.as_str(),
        "\u{10FFFF}",
    ]
    .iter()
    .map(|text| Value::str(text))
    .collect();
    fixture.check_all(&bounds);

    // "a" < "a\0" < "ab": the open interval between "a" and "ab" holds
    // exactly "a\0", and the empty string is the least value.
    let between = fixture.paged(&Excluded(Value::str("a")), &Excluded(Value::str("ab")), 1);
    let a_nul: Vec<RecordId> = fixture
        .model
        .iter()
        .filter(|(_, value)| *value == Value::str("a\0"))
        .map(|(id, _)| *id)
        .collect();
    assert_eq!(between, a_nul);
    assert_eq!(
        fixture
            .paged(&Unbounded, &Included(Value::str("")), 3)
            .len(),
        1
    );
    // Nothing is below the empty string: an excluded "" upper bound is
    // empty for queries and proofs alike.
    assert!(
        fixture
            .paged(&Unbounded, &Excluded(Value::str("")), 3)
            .is_empty()
    );
    assert!(
        fixture
            .proven(&Unbounded, &Excluded(Value::str("")))
            .is_empty()
    );
    // Bytewise order is code point order: upper case sorts first.
    assert_eq!(
        fixture.paged(&Unbounded, &Excluded(Value::str("a")), 5),
        fixture
            .model
            .iter()
            .filter(|(_, value)| *value == Value::str("") || *value == Value::str("Z"))
            .map(|(id, _)| *id)
            .collect::<Vec<_>>()
    );
}

#[test]
fn str_range_bounds_obey_the_indexed_string_limit() {
    let fixture = fixture(&[Value::str("a")]);
    let snapshot = fixture.snapshot();
    let too_long = Value::str(&"x".repeat(arkiv_db::Limits::DEFAULT_INDEXED_LEN + 1));
    for (lower, upper) in [
        (Included(too_long.clone()), Unbounded),
        (Unbounded, Excluded(too_long)),
    ] {
        let query = Query::term(Term::range(name("v"), lower.clone(), upper.clone()));
        let rejected = snapshot
            .query(
                ns(1),
                &query,
                &Projection::ID_ONLY,
                5,
                None,
                Budget::unlimited(),
            )
            .unwrap_err();
        assert_eq!(
            rejected.reason,
            Rejection::InvalidQuery(QueryFault::ValueTooLarge)
        );
        let request = fixture.request(&lower, &upper);
        assert_eq!(
            snapshot
                .prove_range(&request, RangeProofLimits::default(), Budget::unlimited())
                .err(),
            Some(ProofError::InvalidRequest)
        );
    }
    // A configured indexed-string limit binds query bounds and proofs too.
    let limits = Limits {
        max_indexed_len: 4,
        ..Limits::DEFAULT
    };
    let request = fixture.request(&Included(Value::str("abcde")), &Unbounded);
    assert_eq!(
        verify_range(&request, &[], RangeProofLimits::default(), limits).err(),
        Some(ProofError::InvalidRequest)
    );
}

#[test]
fn fixed_byte_ranges_order_as_unsigned_big_endian_strings() {
    let samples: [u8; 7] = [0x80, 0x00, 0xff, 0x7f, 0x01, 0x80, 0xfe];
    let bound_bytes: [u8; 4] = [0x00, 0x7f, 0x80, 0xff];
    type Make = fn(u8) -> Value;
    let makers: [(&str, Make); 6] = [
        ("addr", |b| Value::Addr(spread(b))),
        ("bytes4", |b| Value::Bytes4(spread(b))),
        ("bytes8", |b| Value::Bytes8(spread(b))),
        ("bytes16", |b| Value::Bytes16(spread(b))),
        ("bytes32", |b| Value::Bytes32(spread(b))),
        ("key", |b| Value::Key(spread(b))),
    ];
    for (label, make) in makers {
        let values: Vec<Value> = samples.iter().map(|b| make(*b)).collect();
        let fixture = fixture(&values);
        let bounds: Vec<Value> = bound_bytes.iter().map(|b| make(*b)).collect();
        fixture.check_all(&bounds);
        // A bound that differs only in its last byte splits equal prefixes.
        let mut nudged = model_bytes(&make(0x80));
        *nudged.last_mut().unwrap() ^= 1;
        let nudged = Value::from_term_bytes(make(0).tag(), &nudged).unwrap();
        fixture.check_all(&[nudged]);
        assert!(
            !fixture
                .expected(&Included(make(0x80)), &Unbounded)
                .is_empty(),
            "{label}"
        );
    }
}

/// `n` in the first byte, `!n` in the last, so the first byte decides the
/// order and the tail still varies.
fn spread<const N: usize>(n: u8) -> [u8; N] {
    let mut bytes = [n; N];
    bytes[N - 1] = !n;
    bytes
}

#[test]
fn byte_ordered_types_never_match_another_type() {
    let fixture = fixture(&[Value::Bytes32([1; 32])]);
    // A Key range never returns a Bytes32 attribute of the same bytes.
    assert!(
        fixture
            .paged(&Included(Value::Key([0; 32])), &Unbounded, 5)
            .is_empty()
    );
    assert_eq!(
        fixture
            .paged(&Included(Value::Bytes32([0; 32])), &Unbounded, 5)
            .len(),
        1
    );
}
