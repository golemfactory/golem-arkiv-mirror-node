//! State layout 2: unpadded, prefix-free term keys at the engine level.
//!
//! Strings chosen to break variable-length encodings (the empty string,
//! embedded and trailing NUL and U+0001, multibyte UTF-8, strings that are
//! prefixes of each other, maximum lengths) and every indexable scalar type
//! are written as attributes; equality, range, prefix and `terms` reads, and
//! complete equality and range proofs, are compared with an independent row
//! model (never `Engine::query` or the proof producer). Also: the default
//! profile keeps the 128-byte indexed-string limit while a generic profile
//! admits longer strings up to the 512-byte ceiling, consistently for
//! writes, equality values, range bounds and prefixes; and tampered or
//! misdirected proofs never verify to a different result.
use std::ops::Bound::{self, Excluded, Included, Unbounded};

use arkiv_db::{
    Budget, CellName, Engine, Limits, Projection, Query, RecordId, Row, Snapshot, Term, TypeTag,
    Value,
    proof::{
        EqQuery, EqRequest, ProofLimits, RangeProofLimits, RangeQuery, RangeRequest, verify_eq,
        verify_range,
    },
};

use crate::support::{
    proofs::{
        complete, engine_with_rows, eq_proof, flipped, in_bounds, page, range_proof, snapshot_ref,
    },
    vocab::{OWNER, attr, begin, create_record, name, ns},
    walk::{rows_of, walk},
};

/// Strings whose naive encodings collide or misorder.
fn tricky_strings(max: usize) -> Vec<String> {
    let mut strings: Vec<String> = [
        "",
        "\0",
        "\0\0",
        "\u{1}",
        "\u{1}\0",
        "a",
        "a\0",
        "a\0\0",
        "a\0b",
        "a\u{1}",
        "aa",
        "ab",
        "a\u{7f}",
        "b",
        "é",
        "éa",
        "e\u{301}",
        "€",
        "\u{10ffff}",
        "z",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect();
    strings.push("x".repeat(max));
    strings.push("x".repeat(max - 1));
    strings.push(format!("{}\0", "x".repeat(max - 1)));
    strings.push("\0".repeat(max));
    strings.push("é".repeat(max / 2));
    strings.sort();
    strings.dedup();
    strings
}

/// One namespace, one record per value, all under `attribute`.
fn fixture(limits: Limits, attribute: &str, values: &[Value]) -> (Engine, Vec<Row>) {
    let cells = values
        .iter()
        .map(|value| vec![(name(attribute), attr(value.clone()))]);
    engine_with_rows(limits, "terms", cells)
}

fn query_ids(snapshot: &Snapshot, term: Term) -> Vec<RecordId> {
    let pages = walk(
        snapshot,
        ns(1),
        &Query::term(term),
        &Projection::ALL,
        Budget::unlimited(),
        |_| 7,
        |_| {},
    );
    rows_of(&pages).iter().map(|row| row.id).collect()
}

fn value_of(row: &Row) -> &Value {
    &row.cells[0].1.value
}

fn range_request(
    snapshot: &Snapshot,
    attribute: &str,
    lower: Bound<Value>,
    upper: Bound<Value>,
) -> RangeRequest {
    RangeRequest {
        snapshot: snapshot_ref(snapshot),
        query: RangeQuery {
            namespace: ns(1),
            attribute: name(attribute),
            lower,
            upper,
        },
    }
}

fn range_rows(
    snapshot: &Snapshot,
    limits: Limits,
    attribute: &str,
    lower: Bound<Value>,
    upper: Bound<Value>,
) -> Vec<Row> {
    let request = range_request(snapshot, attribute, lower, upper);
    let bytes = range_proof(snapshot, &request);
    complete(verify_range(&request, &bytes, RangeProofLimits::default(), limits).unwrap()).rows
}

fn eq_request(snapshot: &Snapshot, attribute: &str, value: Value) -> EqRequest {
    EqRequest {
        snapshot: snapshot_ref(snapshot),
        query: EqQuery {
            namespace: ns(1),
            attribute: name(attribute),
            value,
        },
        limit: 64,
        continuation: None,
    }
}

fn eq_rows(snapshot: &Snapshot, limits: Limits, attribute: &str, value: &Value) -> Vec<Row> {
    let request = eq_request(snapshot, attribute, value.clone());
    let bytes = eq_proof(snapshot, &request);
    page(verify_eq(&request, None, &bytes, ProofLimits::default(), limits).unwrap()).rows
}

fn check_strings(limits: Limits) {
    let max = limits.indexed_value_len();
    let strings = tricky_strings(max);
    let values: Vec<Value> = strings.iter().map(|s| Value::str(s)).collect();
    let (engine, model) = fixture(limits, "v", &values);
    let snapshot = engine.head_snapshot();
    // `terms` lists every distinct value in byte order.
    let listed = snapshot
        .terms(
            ns(1),
            &name("v"),
            TypeTag::Str,
            Unbounded,
            200,
            Budget::unlimited(),
        )
        .unwrap()
        .value;
    assert_eq!(listed, values);
    for (index, text) in strings.iter().enumerate() {
        let value = Value::str(text);
        // Equality: exactly one record, by query and by proof.
        assert_eq!(
            query_ids(&snapshot, Term::eq(name("v"), value.clone())),
            vec![model[index].id]
        );
        assert_eq!(
            eq_rows(&snapshot, limits, "v", &value),
            vec![model[index].clone()]
        );
        // Prefix: every string starting with this one.
        let expected: Vec<RecordId> = model
            .iter()
            .filter(|row| matches!(value_of(row), Value::Str(s) if s.starts_with(text.as_str())))
            .map(|row| row.id)
            .collect();
        assert_eq!(
            query_ids(&snapshot, Term::starts_with(name("v"), text)),
            expected,
            "prefix {text:?}"
        );
        // `terms` from an inclusive and exclusive lower bound.
        for (lower, skip) in [(Included(value.clone()), 0), (Excluded(value.clone()), 1)] {
            let listed = snapshot
                .terms(
                    ns(1),
                    &name("v"),
                    TypeTag::Str,
                    lower,
                    200,
                    Budget::unlimited(),
                )
                .unwrap()
                .value;
            assert_eq!(listed, values[index + skip..].to_vec());
        }
    }
    // Ranges over every pair of inclusive/exclusive endpoints from a sample.
    let sample: Vec<Value> = ["", "\0", "a", "a\0", "aa", "é", "z"]
        .into_iter()
        .map(Value::str)
        .chain([values.last().unwrap().clone()])
        .collect();
    let mut bounds = vec![Unbounded];
    for value in &sample {
        bounds.push(Included(value.clone()));
        bounds.push(Excluded(value.clone()));
    }
    for lower in &bounds {
        for upper in &bounds {
            // A range needs at least one bound (queries and proofs alike).
            if *lower == Unbounded && *upper == Unbounded {
                continue;
            }
            let expected: Vec<Row> = model
                .iter()
                .filter(|row| in_bounds(value_of(row), lower, upper))
                .cloned()
                .collect();
            let ids: Vec<RecordId> = expected.iter().map(|row| row.id).collect();
            assert_eq!(
                query_ids(
                    &snapshot,
                    Term::range(name("v"), lower.clone(), upper.clone())
                ),
                ids,
                "{lower:?}..{upper:?}"
            );
            assert_eq!(
                range_rows(&snapshot, limits, "v", lower.clone(), upper.clone()),
                expected,
                "proof {lower:?}..{upper:?}"
            );
        }
    }
}

#[test]
fn default_profile_strings_query_and_prove_like_the_model() {
    check_strings(Limits::DEFAULT);
}

#[test]
fn generic_profile_above_128_bytes_queries_and_proves_like_the_model() {
    let generic = Limits {
        max_indexed_len: 300,
        ..Limits::DEFAULT
    };
    assert_eq!(generic.validate(), Ok(()));
    assert_eq!(generic.indexed_value_len(), 300);
    check_strings(generic);
}

#[test]
fn indexed_string_admission_is_consistent_per_profile() {
    let generic = Limits {
        max_indexed_len: Value::INDEXED_LEN_CEILING,
        ..Limits::DEFAULT
    };
    for (limits, admitted) in [
        (Limits::DEFAULT, 128),
        (generic, Value::INDEXED_LEN_CEILING),
    ] {
        let (engine, _) = fixture(limits, "v", &[]);
        let snapshot = engine.head_snapshot();
        for (len, accepted) in [(admitted, true), (admitted + 1, false)] {
            // UTF-8 bytes, not characters: a two-byte character over the edge.
            for text in ["x".repeat(len), format!("{}é", "x".repeat(len - 2))] {
                let value = Value::str(&text);
                let cells = vec![(name("v"), attr(value.clone()))];
                let write = begin(&engine, None).execute(
                    OWNER,
                    create_record(ns(1), "k", cells),
                    Budget::unlimited(),
                );
                assert_eq!(write.is_ok(), accepted, "write {} bytes", text.len());
                for term in [
                    Term::eq(name("v"), value.clone()),
                    Term::starts_with(name("v"), &text),
                    Term::range(name("v"), Included(value.clone()), Unbounded),
                ] {
                    let read = snapshot.query(
                        ns(1),
                        &Query::term(term),
                        &Projection::ALL,
                        10,
                        None,
                        Budget::unlimited(),
                    );
                    assert_eq!(read.is_ok(), accepted, "query {} bytes", text.len());
                }
                let eq = eq_request(&snapshot, "v", value.clone());
                assert_eq!(
                    snapshot
                        .prove_eq(&eq, ProofLimits::default(), Budget::unlimited())
                        .is_ok(),
                    accepted
                );
                let range = range_request(&snapshot, "v", Included(value.clone()), Unbounded);
                assert_eq!(
                    snapshot
                        .prove_range(&range, RangeProofLimits::default(), Budget::unlimited())
                        .is_ok(),
                    accepted
                );
            }
        }
    }
    // Above the ceiling no instance can be configured.
    let over = Limits {
        max_indexed_len: Value::INDEXED_LEN_CEILING + 1,
        ..Limits::DEFAULT
    };
    assert!(over.validate().is_err());
}

#[test]
fn every_scalar_type_orders_and_proves_like_the_model() {
    let mut min_dec = [0; 32];
    min_dec[0] = 0x80;
    let mut max_dec = [0xff; 32];
    max_dec[0] = 0x7f;
    let groups: Vec<Vec<Value>> = vec![
        vec![Value::Bool(true), Value::Bool(false)],
        vec![
            Value::I64(i64::MIN),
            Value::I64(-1),
            Value::I64(0),
            Value::I64(i64::MAX),
        ],
        vec![
            Value::U64(0),
            Value::U64(1),
            Value::U64(256),
            Value::U64(u64::MAX),
        ],
        vec![Value::I32(-5), Value::I32(5), Value::I32(i32::MIN)],
        vec![Value::U32(0), Value::U32(u32::MAX)],
        vec![Value::U128(0), Value::U128(u128::MAX), Value::U128(1 << 64)],
        vec![
            Value::I128(i128::MIN),
            Value::I128(0),
            Value::I128(i128::MAX),
        ],
        vec![
            Value::U256([0; 32]),
            Value::U256([0xff; 32]),
            Value::U256([1; 32]),
        ],
        vec![
            Value::I256(min_dec),
            Value::I256([0; 32]),
            Value::I256(max_dec),
        ],
        vec![
            Value::Dec(min_dec),
            Value::Dec([0xff; 32]),
            Value::Dec(max_dec),
        ],
        vec![Value::Dec32(-1), Value::Dec32(1)],
        vec![Value::Dec64(i64::MIN), Value::Dec64(7)],
        vec![Value::Dec128(-7), Value::Dec128(7)],
        vec![
            Value::f64(-1.5).unwrap(),
            Value::f64(0.0).unwrap(),
            Value::f64(f64::INFINITY).unwrap(),
            Value::f64(f64::NEG_INFINITY).unwrap(),
        ],
        vec![Value::f32(-0.5).unwrap(), Value::f32(2.0).unwrap()],
        vec![Value::Date32(-1), Value::Date32(1)],
        vec![Value::Timestamp64(-1), Value::Timestamp64(i64::MAX)],
        vec![Value::Addr([0; 20]), Value::Addr([0xff; 20])],
        vec![Value::Bytes4([0; 4]), Value::Bytes4([0, 0, 0, 1])],
        vec![Value::Bytes8([9; 8]), Value::Bytes8([0; 8])],
        vec![Value::Bytes16([0xff; 16]), Value::Bytes16([0; 16])],
        vec![Value::Bytes32([0; 32]), Value::Bytes32([0xfe; 32])],
        vec![Value::Key([1; 32]), Value::Key([0; 32])],
    ];
    for values in groups {
        let (engine, model) = fixture(Limits::DEFAULT, "v", &values);
        let snapshot = engine.head_snapshot();
        let tag = values[0].tag();
        let mut sorted = values.clone();
        sorted.sort_by(|a, b| a.cmp_ordered(b).unwrap());
        let listed = snapshot
            .terms(ns(1), &name("v"), tag, Unbounded, 100, Budget::unlimited())
            .unwrap()
            .value;
        assert_eq!(listed, sorted, "{tag:?}");
        for value in &values {
            let expected: Vec<Row> = model
                .iter()
                .filter(|row| value_of(row) == value)
                .cloned()
                .collect();
            assert_eq!(eq_rows(&snapshot, Limits::DEFAULT, "v", value), expected);
            for (lower, upper) in [
                (Included(value.clone()), Unbounded),
                (Excluded(value.clone()), Unbounded),
                (Unbounded, Excluded(value.clone())),
                (Unbounded, Included(value.clone())),
            ] {
                let expected: Vec<Row> = model
                    .iter()
                    .filter(|row| in_bounds(value_of(row), &lower, &upper))
                    .cloned()
                    .collect();
                assert_eq!(
                    range_rows(
                        &snapshot,
                        Limits::DEFAULT,
                        "v",
                        lower.clone(),
                        upper.clone()
                    ),
                    expected,
                    "{tag:?} {lower:?}..{upper:?}"
                );
            }
        }
    }
}

#[test]
fn tampered_or_misdirected_proofs_never_verify_to_another_result() {
    let values: Vec<Value> = ["a", "a\0", "aa", "ab", "b"]
        .into_iter()
        .map(Value::str)
        .collect();
    let (engine, model) = fixture(Limits::DEFAULT, "v", &values);
    let snapshot = engine.head_snapshot();
    // Equality for "a": a different attribute, type or value must not accept
    // the proof of another request.
    let honest = eq_request(&snapshot, "v", Value::str("a"));
    let bytes = eq_proof(&snapshot, &honest);
    let verify = |request: &EqRequest, bytes: &[u8]| {
        verify_eq(
            request,
            None,
            bytes,
            ProofLimits::default(),
            Limits::DEFAULT,
        )
    };
    let verified = verify(&honest, &bytes).unwrap();
    assert_eq!(page(verified.clone()).rows, vec![model[0].clone()]);
    for other in [
        eq_request(&snapshot, "w", Value::str("a")),
        eq_request(&snapshot, "v", Value::str("aa")),
        eq_request(&snapshot, "v", Value::str("a\0")),
        eq_request(&snapshot, "v", Value::Bool(true)),
        EqRequest {
            query: EqQuery {
                namespace: ns(2),
                ..honest.query.clone()
            },
            ..honest.clone()
        },
    ] {
        assert!(verify(&other, &bytes).is_err());
    }
    // Every single-byte change either fails or gives the honest answer.
    for index in 0..bytes.len() {
        if let Ok(result) = verify(&honest, &flipped(&bytes, index, 0x01)) {
            assert_eq!(result, verified, "byte {index}");
        }
    }
    // A complete range ["a", "aa") holds exactly "a" and "a\0"; dropping or
    // altering any witness byte never yields another successful answer.
    let request = range_request(
        &snapshot,
        "v",
        Included(Value::str("a")),
        Excluded(Value::str("aa")),
    );
    let bytes = range_proof(&snapshot, &request);
    let verify = |request: &RangeRequest, bytes: &[u8]| {
        verify_range(request, bytes, RangeProofLimits::default(), Limits::DEFAULT)
    };
    let verified = verify(&request, &bytes).unwrap();
    assert_eq!(complete(verified.clone()).rows, model[..2].to_vec());
    for index in 0..bytes.len() {
        if let Ok(result) = verify(&request, &flipped(&bytes, index, 0x80)) {
            assert_eq!(result, verified, "byte {index}");
        }
        assert!(verify(&request, &bytes[..index]).is_err());
    }
    // The same witness never verifies for a wider, narrower or retyped range.
    for (lower, upper) in [
        (Included(Value::str("a")), Included(Value::str("aa"))),
        (Excluded(Value::str("a")), Excluded(Value::str("aa"))),
        (Included(Value::str("")), Excluded(Value::str("aa"))),
        (Included(Value::Bool(false)), Unbounded),
    ] {
        let other = RangeRequest {
            query: RangeQuery {
                lower,
                upper,
                ..request.query.clone()
            },
            ..request.clone()
        };
        assert!(verify(&other, &bytes).is_err());
    }
}

/// The worst-case term keys: a 32-byte attribute name and indexed strings
/// at the 512-byte ceiling (548-byte term keys), alone and sharing long
/// prefixes (so leaves and extensions carry near-maximal paths), all queried
/// and proven at the generic ceiling profile.
#[test]
fn ceiling_profile_longest_names_and_values_query_and_prove() {
    let limits = Limits {
        max_indexed_len: Value::INDEXED_LEN_CEILING,
        ..Limits::DEFAULT
    };
    let max = limits.indexed_value_len();
    let attribute = "n".repeat(CellName::MAX_LEN);
    let long = "x".repeat(max);
    let strings: Vec<String> = vec![
        String::new(),
        "x".repeat(max - 1),
        format!("{}\0", "x".repeat(max - 1)),
        format!("{}y", "x".repeat(max - 1)),
        long.clone(),
        "\0".repeat(max),
        "\u{10ffff}".repeat(max / 4),
    ];
    // One record each, plus a single-entry namespace whose term map is one
    // leaf holding the whole 548-byte key.
    for texts in [strings, vec![long]] {
        let values: Vec<Value> = texts.iter().map(|text| Value::str(text)).collect();
        let (engine, model) = fixture(limits, &attribute, &values);
        let snapshot = engine.head_snapshot();
        for (index, value) in values.iter().enumerate() {
            assert_eq!(
                eq_rows(&snapshot, limits, &attribute, value),
                vec![model[index].clone()]
            );
        }
        let request = range_request(&snapshot, &attribute, Included(Value::str("")), Unbounded);
        let bytes = range_proof(&snapshot, &request);
        let verified = verify_range(&request, &bytes, RangeProofLimits::default(), limits);
        let mut rows = complete(verified.unwrap()).rows;
        let mut expected = model.clone();
        expected.sort_by(|a, b| value_of(a).cmp_ordered(value_of(b)).unwrap());
        rows.sort_by(|a, b| value_of(a).cmp_ordered(value_of(b)).unwrap());
        assert_eq!(rows, expected);
        // The default-profile verifier bounds term keys at 164 bytes, so it
        // rejects these proofs instead of misreading them.
        assert!(
            verify_range(
                &request,
                &bytes,
                RangeProofLimits::default(),
                Limits::DEFAULT
            )
            .is_err()
        );
    }
}
