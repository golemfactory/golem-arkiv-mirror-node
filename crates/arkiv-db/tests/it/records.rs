//! The typed record model: keys, ids, names, values and cells; the three
//! record writes; and `get_record`, under model version 3.
//!
//! The frozen encodings (`RecordKey` as `len ‖ bytes`, 2 to 33 bytes; term
//! bytes; `TypeTag` ids; the `CellName` grammar) are literal vectors. Rows
//! and term counts come from a `BTreeMap` reference model, never from the
//! engine's index; every cost is a literal derived by hand next to it. The
//! every-budget sweeps of the create, patch and delete vectors are in
//! `query_metering.rs`; query evaluation and cursors are in `queries.rs` and
//! `pagination.rs`.

use std::cmp::Ordering;

use arkiv_db::{
    ArgumentFault, Branch, Budget, CellName, Cost, DEFAULT_NAMESPACE_CAP, Effect, Engine, Ledger,
    Limits, NameFault, NamespaceId, PhysicalTrace, PriceSchedule, Projection, Query, QueryFault,
    RecordId, RecordKey, Rejected, Rejection, Revision, SnapshotId, TypeTag, Value, WriteOp,
};

use crate::support::{
    model::Model,
    reader::Reader,
    vocab::{
        self, ALICE, BOB, Cells, applied, apply, attr, begin, create_record, delete, delete_record,
        engine_with_namespace, entry, field, key, name, patch_record, publish, put, read_admission,
        read_applied, read_rejected, reject, rejected, rid, transfer,
    },
};

// FIXTURE unit prices of the classes this file charges, for ledger lines and
// the derivations below.
const DIRECTORY_LOOKUP: u64 = 1;
const NAMESPACE_TRANSFER: u64 = 20;
const ROOT_UPDATE: u64 = 2;

// The worked fixture: records a, b, c created in that order, each with
// attributes kind: Str, price: I64, $expiresAt: U64(5) and a field
// note: Bytes("xyz"); kinds book, book, pen; prices 10, 20, 5.
//
// Name order (bytewise, `$` = 0x24 sorts before letters): $expiresAt, kind,
// note, price. Cell bytes = name bytes + encoded value bytes:
//   $expiresAt: 10 + 8 = 18   kind "book": 4 + 4 = 8   kind "pen": 4 + 3 = 7
//   note: 4 + 3 = 7           price: 5 + 8 = 13
// so b(a) = 18 + 8 + 7 + 13 = 46, b(b) = 46, b(c) = 45; c = 4 cells, a = 3
// attribute cells (6 terms: a value term and an existence term each). The
// stored row adds the 1-byte key: n(a) = n(b) = 47, n(c) = 46 (RowByte).
// Every record group also charges RootUpdate 3 (key binding, row, catalog
// leaf) and every index group RootUpdate 2 (posting entry, term leaf), 2
// units each.

/// create a, every term new: 1 + 3 + (20 + 4*4 + 46 + RowByte 47 +
/// RootUpdate 3*2) + 6*(6 + 2*2) + 6*30 = 1 + 3 + 135 + 60 + 180 = 379.
const CREATE_A: u64 = 379;
/// create b, only the value term price=20 new: 1 + 3 + 135 + 60 + 30 = 229.
const CREATE_B: u64 = 229;
/// create c, kind=pen and price=5 new: 1 + 3 + (20 + 16 + 45 + 46 + 6) + 60
/// + 60 = 257.
const CREATE_C: u64 = 257;
/// Distinct terms after a, b, c: kind {exist, book, pen}, price {exist, 10,
/// 20, 5}, $expiresAt {exist, 5} = 9.
const ABC_TERMS: usize = 9;

/// The four cells of a fixture record.
fn fixture_cells(kind: &str, price: i64) -> Cells {
    vec![
        (name("kind"), attr(Value::str(kind))),
        (name("price"), attr(Value::I64(price))),
        (name("$expiresAt"), attr(Value::U64(5))),
        (name("note"), field(Value::bytes(b"xyz"))),
    ]
}

/// An admission rejection: nothing charged, nothing visited.
fn admission(reason: Rejection) -> Rejected {
    Rejected {
        reason,
        receipt: rejected(0, reason),
        physical: PhysicalTrace::default(),
    }
}

/// Creates a, b, c on `branch`, asserting the worked receipts, and mirrors
/// them into `model`.
fn create_abc(branch: &mut Branch, ns: NamespaceId, model: &mut Model) {
    let records = [
        ("a", "book", 10, CREATE_A),
        ("b", "book", 20, CREATE_B),
        ("c", "pen", 5, CREATE_C),
    ];
    for (ordinal, (text, kind, price, cost)) in (1..).zip(records) {
        let cells = fixture_cells(kind, price);
        let executed = apply(branch, ALICE, create_record(ns, text, cells.clone()));
        assert_eq!(executed.effect, Effect::RecordCreated { id: rid(ordinal) });
        assert_eq!(executed.receipt, applied(cost), "create {text}");
        assert_eq!(model.create(text, &cells), rid(ordinal));
    }
}

/// Snapshot 2: the fixture namespace holding a, b, c (ids 1, 2, 3).
fn fixture_abc() -> (Engine, NamespaceId, Model) {
    let (mut engine, ns) = engine_with_namespace(Limits::DEFAULT);
    let mut model = Model::new();
    let mut branch = begin(&engine, None);
    create_abc(&mut branch, ns, &mut model);
    assert_eq!(publish(&mut engine, branch).sequence(), 2);
    (engine, ns, model)
}

/// The unmetered statistics of a namespace's typed state.
#[derive(Clone, Debug, PartialEq, Eq)]
struct TypedStats {
    records: Option<usize>,
    terms: Option<usize>,
    consistent: Option<bool>,
}

fn stats(view: &impl Reader, ns: NamespaceId) -> TypedStats {
    TypedStats {
        records: view.typed_record_count(ns),
        terms: view.term_count(ns),
        consistent: view.index_consistent(ns),
    }
}

/// The stats the model implies for a consistent namespace.
fn expected_stats(model: &Model) -> TypedStats {
    TypedStats {
        records: Some(model.len()),
        terms: Some(model.term_count()),
        consistent: Some(true),
    }
}

/// Asserts that `view` holds exactly the model's records (by key, full
/// projection) and its statistics.
fn assert_matches_model(view: &impl Reader, ns: NamespaceId, model: &Model, keys: &[&str]) {
    assert_eq!(stats(view, ns), expected_stats(model));
    for text in keys {
        let read = view
            .get_record(ns, &key(text), &Projection::ALL, Budget::unlimited())
            .unwrap_or_else(|rejected| panic!("unlimited get_record {text}: {rejected}"));
        assert_eq!(
            read.value,
            model.row(text, &Projection::ALL),
            "record {text}"
        );
    }
}

#[test]
fn record_keys_take_1_to_32_bytes_and_encode_as_length_and_bytes() {
    assert_eq!(RecordKey::MAX_LEN, 32);
    assert_eq!(RecordKey::new(b""), None, "empty keys are refused");
    assert_eq!(RecordKey::new(&[7u8; 33]), None, "33-byte keys are refused");
    // The one canonical form, in the codec and as the key-binding trie key:
    // `len ‖ bytes`, no padding. b"a" -> 01 61.
    let short = RecordKey::new(b"a").expect("one byte is a key");
    assert_eq!(short.as_bytes(), b"a");
    assert_eq!(arkiv_db::encode(&short), [0x01, 0x61]);
    // 31 zero bytes differ from one zero byte in the length byte.
    let zeros = RecordKey::new(&[0u8; 31]).expect("31 bytes are a key");
    let one_zero = RecordKey::new(&[0u8]).expect("one zero byte is a key");
    assert_ne!(one_zero, zeros);
    assert_eq!(arkiv_db::encode(&one_zero), [0x01, 0x00]);
    // Every accepted length round-trips.
    for len in 1..=32usize {
        let bytes: Vec<u8> = (0..len).map(|i| u8::try_from(i).unwrap() + 1).collect();
        let key = RecordKey::new(&bytes).unwrap_or_else(|| panic!("{len} bytes are a key"));
        let encoded = arkiv_db::encode(&key);
        assert_eq!(usize::from(encoded[0]), len);
        assert_eq!(&encoded[1..], bytes.as_slice());
        assert_eq!(arkiv_db::decode_exact::<RecordKey>(&encoded).unwrap(), key);
    }
}

#[test]
fn cell_names_follow_the_grammar_and_order_bytewise() {
    assert_eq!(CellName::MAX_LEN, 32);
    for good in [
        "a",
        "Z",
        "A1",
        "$owner",
        "$e",
        "x_y-z.w:v",
        "kind",
        &"n".repeat(20),
    ] {
        let name = CellName::new(good).unwrap_or_else(|fault| panic!("{good:?}: {fault}"));
        assert_eq!(name.as_str(), good);
        assert_eq!(name.len(), good.len());
        assert!(!name.is_empty());
    }
    assert_eq!(CellName::new(""), Err(NameFault::Empty));
    assert_eq!(
        CellName::new(&"n".repeat(CellName::MAX_LEN + 1)),
        Err(NameFault::TooLong)
    );
    for bad in [
        "$", "$$a", "$1", "$_", "1a", "_a", "-a", ".a", ":a", "a b", " a", "a#b", "a@b", "é",
        "a/b", "a\0", "a\n",
    ] {
        assert_eq!(CellName::new(bad), Err(NameFault::Grammar), "{bad:?}");
    }
    // Case-sensitive, no normalisation; Ord is bytewise so upper case sorts first.
    assert_ne!(name("Kind"), name("kind"));
    assert!(name("B") < name("a"));
    assert!(
        name("$z") < name("a"),
        "`$` (0x24) sorts before every letter"
    );
    assert!(name("abc") < name("abd"));
    assert!(name("ab") < name("abc"));
}

#[test]
fn type_tags_have_frozen_ids_and_capabilities() {
    let ids: Vec<u8> = TypeTag::ALL[..4].iter().map(|tag| tag.id()).collect();
    assert_eq!(ids, vec![1, 2, 3, 4]);
    assert_eq!(
        [
            TypeTag::Bool,
            TypeTag::Str,
            TypeTag::Bytes,
            TypeTag::U64,
            TypeTag::I64
        ]
        .map(TypeTag::id),
        [1, 2, 3, 13, 17]
    );
    let unindexed: Vec<TypeTag> = TypeTag::ALL
        .into_iter()
        .filter(|t| !t.is_indexable())
        .collect();
    assert_eq!(unindexed, vec![TypeTag::Bytes]);
    // Ordered is the same set as indexable: every attribute type.
    for tag in TypeTag::ALL {
        assert_eq!(tag.is_ordered(), tag.is_indexable(), "{tag}");
    }
    let prefix: Vec<TypeTag> = TypeTag::ALL
        .into_iter()
        .filter(|t| t.supports_prefix())
        .collect();
    assert_eq!(prefix, vec![TypeTag::Str]);
}

#[test]
fn values_have_frozen_term_bytes_and_encoded_lengths() {
    let term = |value: &Value| value.term_bytes().expect("indexable");
    assert_eq!(
        term(&Value::I64(-1)),
        vec![0x7f, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff]
    );
    assert_eq!(term(&Value::I64(0)), vec![0x80, 0, 0, 0, 0, 0, 0, 0]);
    assert_eq!(term(&Value::I64(i64::MIN)), vec![0u8; 8]);
    assert_eq!(term(&Value::I64(i64::MAX)), vec![0xff; 8]);
    assert_eq!(term(&Value::U64(258)), vec![0, 0, 0, 0, 0, 0, 0x01, 0x02]);
    assert_eq!(term(&Value::str("ab")), vec![0x61, 0x62]);
    assert_eq!(term(&Value::Bool(false)), vec![0x00]);
    assert_eq!(term(&Value::Bool(true)), vec![0x01]);
    assert_eq!(Value::bytes(b"ab").term_bytes(), None, "Bytes has no term");

    // The I64 encoding orders as the integers do: sort a sample by term bytes
    // and compare with the numeric order, independently of the engine.
    let sample = [i64::MAX, 1, i64::MIN, -1, 0, -2, 2, 1 << 40, -(1 << 40)];
    let mut by_bytes: Vec<i64> = sample.to_vec();
    by_bytes.sort_by_key(|value| term(&Value::I64(*value)));
    let mut numeric = sample.to_vec();
    numeric.sort_unstable();
    assert_eq!(by_bytes, numeric);

    assert_eq!(Value::Bool(true).encoded_len(), 1);
    assert_eq!(Value::I64(-5).encoded_len(), 8);
    assert_eq!(Value::U64(5).encoded_len(), 8);
    assert_eq!(Value::str("").encoded_len(), 0);
    assert_eq!(
        Value::str("héllo").encoded_len(),
        6,
        "UTF-8 byte length, not chars"
    );
    assert_eq!(Value::bytes(b"xyz").encoded_len(), 3);

    assert_eq!(
        Value::I64(-1).cmp_ordered(&Value::I64(1)),
        Some(Ordering::Less)
    );
    assert_eq!(
        Value::U64(2).cmp_ordered(&Value::U64(2)),
        Some(Ordering::Equal)
    );
    assert_eq!(
        Value::I64(1).cmp_ordered(&Value::U64(1)),
        None,
        "tags differ"
    );
    assert_eq!(
        Value::str("a").cmp_ordered(&Value::str("b")),
        Some(Ordering::Less),
        "Str orders by its UTF-8 bytes"
    );
    assert_eq!(
        Value::Bool(false).cmp_ordered(&Value::Bool(true)),
        Some(Ordering::Less)
    );
    assert_eq!(Value::bytes(b"a").cmp_ordered(&Value::bytes(b"b")), None);
    assert_ne!(
        Value::I64(1),
        Value::U64(1),
        "values of different types never compare equal"
    );
}

#[test]
fn limits_are_fixed_per_engine() {
    let defaults = Limits {
        namespace_cap: 1 << 32,
        max_cells: 64,
        max_str_len: 4096,
        max_bytes_len: 4096,
        max_page: 200,
        max_groups: 8,
        max_literals: 8,
        max_cell_name_len: CellName::MAX_LEN,
        max_indexed_len: arkiv_db::Limits::DEFAULT_INDEXED_LEN,
    };
    assert_eq!(Limits::DEFAULT, defaults);
    assert_eq!(DEFAULT_NAMESPACE_CAP, 1 << 32);
    assert_eq!(Engine::new(PriceSchedule::FIXTURE).limits(), defaults);
    assert_eq!(
        Engine::with_namespace_cap(PriceSchedule::FIXTURE, 5).limits(),
        Limits {
            namespace_cap: 5,
            ..defaults
        }
    );
    let custom = Limits {
        max_cells: 4,
        max_page: 3,
        ..defaults
    };
    let engine = Engine::with_limits(PriceSchedule::FIXTURE, custom);
    assert_eq!(engine.limits(), custom);
    assert_eq!(begin(&engine, None).limits(), custom);
}

const ABC_KEYS: [&str; 3] = ["a", "b", "c"];

/// `count` distinct names `c00`.., each an indexed `Bool`.
fn many_cells(count: usize) -> Cells {
    (0..count)
        .map(|i| (name(&format!("c{i:02}")), attr(Value::Bool(true))))
        .collect()
}

#[test]
fn argument_faults_are_admission_rejections_with_nothing_recorded() {
    let (engine, ns, model) = fixture_abc();
    let mut branch = begin(&engine, None);
    let big_str = "x".repeat(4097);
    let big_bytes = vec![0u8; 4097];
    let ok = attr(Value::Bool(true));
    let bytes_attr = attr(Value::bytes(b"x"));
    use ArgumentFault::{
        BytesNotIndexable, DuplicateCell, SetAndRemoveOverlap, TooManyCells, ValueTooLarge,
    };
    let cases: Vec<(&str, WriteOp, ArgumentFault)> = vec![
        (
            "create: 65 cells",
            create_record(ns, "d", many_cells(65)),
            TooManyCells,
        ),
        (
            "create: duplicate name",
            create_record(
                ns,
                "d",
                vec![(name("k"), ok.clone()), (name("k"), ok.clone())],
            ),
            DuplicateCell,
        ),
        (
            "create: Str field of 4097 bytes",
            create_record(ns, "d", vec![(name("v"), field(Value::str(&big_str)))]),
            ValueTooLarge,
        ),
        (
            "create: Str attribute of 4097 bytes",
            create_record(ns, "d", vec![(name("v"), attr(Value::str(&big_str)))]),
            ValueTooLarge,
        ),
        (
            "create: Bytes field of 4097 bytes",
            create_record(ns, "d", vec![(name("v"), field(Value::bytes(&big_bytes)))]),
            ValueTooLarge,
        ),
        (
            "create: Bytes attribute",
            create_record(ns, "d", vec![(name("v"), bytes_attr.clone())]),
            BytesNotIndexable,
        ),
        // Per-cell order: DuplicateCell, ValueTooLarge, BytesNotIndexable;
        // the first faulting cell in submitted order wins, and TooManyCells
        // precedes every per-cell check.
        (
            "create: duplicate beats bytes attribute in the second cell",
            create_record(
                ns,
                "d",
                vec![(name("k"), ok.clone()), (name("k"), bytes_attr.clone())],
            ),
            DuplicateCell,
        ),
        (
            "create: too large beats bytes attribute in one cell",
            create_record(ns, "d", vec![(name("v"), attr(Value::bytes(&big_bytes)))]),
            ValueTooLarge,
        ),
        (
            "create: bytes attribute in the first cell beats a later duplicate",
            create_record(
                ns,
                "d",
                vec![
                    (name("a"), bytes_attr.clone()),
                    (name("b"), ok.clone()),
                    (name("b"), ok.clone()),
                ],
            ),
            BytesNotIndexable,
        ),
        (
            "create: too many cells beats a bytes attribute",
            create_record(ns, "d", {
                let mut cells = many_cells(64);
                cells.insert(0, (name("v"), bytes_attr.clone()));
                cells
            }),
            TooManyCells,
        ),
        (
            "patch: 33 set + 32 remove names",
            patch_record(
                ns,
                "a",
                many_cells(33),
                (33..65).map(|i| name(&format!("c{i:02}"))).collect(),
            ),
            TooManyCells,
        ),
        (
            "patch: too many names beats a bytes attribute in set",
            patch_record(
                ns,
                "a",
                {
                    let mut cells = many_cells(64);
                    cells.insert(0, (name("v"), bytes_attr.clone()));
                    cells
                },
                vec![name("w")],
            ),
            TooManyCells,
        ),
        (
            "patch: duplicate in set",
            patch_record(
                ns,
                "a",
                vec![(name("k"), ok.clone()), (name("k"), ok.clone())],
                vec![],
            ),
            DuplicateCell,
        ),
        (
            "patch: bytes attribute in set",
            patch_record(ns, "a", vec![(name("v"), bytes_attr.clone())], vec![]),
            BytesNotIndexable,
        ),
        (
            "patch: set is validated before the remove list",
            patch_record(
                ns,
                "a",
                vec![(name("v"), bytes_attr)],
                vec![name("w"), name("w")],
            ),
            BytesNotIndexable,
        ),
        (
            "patch: duplicate in remove",
            patch_record(ns, "a", vec![], vec![name("k"), name("k")]),
            DuplicateCell,
        ),
        (
            "patch: duplicate in remove beats the overlap",
            patch_record(
                ns,
                "a",
                vec![(name("k"), ok.clone())],
                vec![name("k"), name("k")],
            ),
            DuplicateCell,
        ),
        (
            "patch: a name in both lists",
            patch_record(ns, "a", vec![(name("k"), ok)], vec![name("k")]),
            SetAndRemoveOverlap,
        ),
    ];
    for (label, op, fault) in cases {
        let expected = admission(Rejection::InvalidArgument(fault));
        for budget in [
            Budget::unlimited(),
            Budget::limit(0),
            Budget::limit(1_000_000),
        ] {
            assert_eq!(
                branch.estimate(ALICE, op.clone(), budget),
                Err(expected.clone()),
                "{label}"
            );
            assert_eq!(
                branch.execute(ALICE, op.clone(), budget),
                Err(expected.clone()),
                "{label}"
            );
        }
        // A traced budget adds an empty ledger to the zero-cost receipt.
        let traced_rejection = branch
            .execute(ALICE, op, Budget::unlimited().traced())
            .expect_err(label);
        assert_eq!(traced_rejection.receipt.spent, Cost::ZERO, "{label}");
        assert_eq!(
            traced_rejection.receipt.ledger,
            Some(Ledger::default()),
            "{label}"
        );
    }
    assert!(
        branch.receipts().is_empty(),
        "admission faults are never recorded"
    );
    assert_eq!(branch.spent(), Cost::ZERO);
    assert_matches_model(&branch, ns, &model, &ABC_KEYS);

    // The boundaries are admitted: a 4096-byte Str field costs 1 + 3 + {20 +
    // 4 + CellByte (1 + 4096) + RowByte (1 + 4097) + RootUpdate 3*2} = 8229;
    // 64 indexed Bool cells cost 1 + 3 + {20 + 64*4 + 64*(3 + 1) + RowByte
    // (1 + 256) + RootUpdate 3*2} + 128 new terms * (36 + RootUpdate 2*2) =
    // 4 + 795 + 5120 = 5919.
    let boundary = vec![(name("v"), field(Value::str(&"x".repeat(4096))))];
    let executed = apply(&mut branch, ALICE, create_record(ns, "d", boundary));
    assert_eq!(executed.receipt, applied(8229));
    let executed = apply(&mut branch, ALICE, create_record(ns, "e", many_cells(64)));
    assert_eq!(executed.receipt, applied(5919));
    assert_eq!(executed.effect, Effect::RecordCreated { id: rid(5) });
}

#[test]
fn argument_validation_precedes_every_charge_and_the_block_budget_check() {
    let (engine, ns, model) = fixture_abc();
    let invalid = create_record(ns, "d", vec![(name("v"), attr(Value::bytes(b"x")))]);
    let bad_args = admission(Rejection::InvalidArgument(ArgumentFault::BytesNotIndexable));
    let valid = create_record(ns, "d", vec![(name("v"), attr(Value::Bool(true)))]);

    // Block budget 10: a valid op with budget 20 is ExceedsBlockBudget; an
    // invalid one is refused for its arguments first. Neither is recorded.
    let mut branch = begin(&engine, Some(10));
    assert_eq!(
        branch.execute(ALICE, invalid.clone(), Budget::limit(20)),
        Err(bad_args.clone())
    );
    assert_eq!(
        branch.execute(ALICE, valid.clone(), Budget::limit(20)),
        Err(admission(Rejection::ExceedsBlockBudget {
            remaining: Cost::new(10)
        }))
    );
    assert!(branch.receipts().is_empty());

    let mut branch = begin(&engine, None);
    // Before the directory charge: an unknown namespace and a non-owner both
    // see the argument fault, not UnknownNamespace or Unauthorized.
    let elsewhere = create_record(
        vocab::ns(9),
        "d",
        vec![(name("v"), attr(Value::bytes(b"x")))],
    );
    assert_eq!(
        branch.execute(ALICE, elsewhere, Budget::unlimited()),
        Err(bad_args.clone())
    );
    assert_eq!(
        branch.execute(BOB, invalid.clone(), Budget::unlimited()),
        Err(bad_args.clone())
    );
    // Before the first charge: a zero budget still reports the fault, while
    // the same zero budget on valid arguments is a recorded OutOfBudget.
    assert_eq!(
        branch.execute(ALICE, invalid, Budget::limit(0)),
        Err(bad_args)
    );
    assert!(branch.receipts().is_empty());
    let starved = branch
        .execute(ALICE, valid, Budget::limit(0))
        .expect_err("nothing fits a zero budget");
    assert_eq!(starved.receipt, rejected(0, Rejection::OutOfBudget));
    assert_eq!(branch.receipts(), &[starved.receipt]);
    assert_eq!(branch.spent(), Cost::ZERO);
    assert_matches_model(&branch, ns, &model, &ABC_KEYS);
}

#[test]
fn ids_are_dense_and_never_reused_across_delete_and_recreate() {
    let (mut engine, ns, mut model) = fixture_abc();
    let mut branch = begin(&engine, None);
    let deleted = apply(&mut branch, ALICE, delete_record(ns, "a"));
    assert_eq!(deleted.effect, Effect::RecordDeleted { id: rid(1) });
    assert_eq!(model.delete("a"), rid(1));
    // The same key again: a new id, and price=10 (a's alone) is new again:
    // 379 - 6*30 + 30 = 229.
    let recreated = apply(
        &mut branch,
        ALICE,
        create_record(ns, "a", fixture_cells("book", 10)),
    );
    assert_eq!(recreated.effect, Effect::RecordCreated { id: rid(4) });
    assert_eq!(recreated.receipt, applied(229));
    assert_eq!(model.create("a", &fixture_cells("book", 10)), rid(4));
    let read = branch
        .get_record(ns, &key("a"), &Projection::ID_ONLY, Budget::unlimited())
        .expect("reads");
    assert_eq!(read.value, model.row("a", &Projection::ID_ONLY));
    assert_eq!(read.receipt, read_applied(7));

    // Rejected creates mint nothing: a bound key and a starved budget.
    let bound = reject(
        &mut branch,
        ALICE,
        create_record(ns, "b", fixture_cells("book", 20)),
    );
    assert_eq!(bound.reason, Rejection::AlreadyExists);
    let starved = branch
        .execute(
            ALICE,
            create_record(ns, "e", fixture_cells("pen", 1)),
            Budget::limit(4),
        )
        .expect_err("the record group needs 133 more");
    assert_eq!(starved.receipt, rejected(4, Rejection::OutOfBudget));
    let next = apply(
        &mut branch,
        ALICE,
        create_record(ns, "d", fixture_cells("pen", 1)),
    );
    assert_eq!(next.effect, Effect::RecordCreated { id: rid(5) });
    model.create("d", &fixture_cells("pen", 1));
    assert_matches_model(&branch, ns, &model, &["a", "b", "c", "d", "e"]);
    publish(&mut engine, branch);

    // Across blocks the allocator continues; a deleted id stays retired.
    let mut branch = begin(&engine, None);
    let deleted = apply(&mut branch, ALICE, delete_record(ns, "a"));
    assert_eq!(deleted.effect, Effect::RecordDeleted { id: rid(4) });
    model.delete("a");
    let recreated = apply(
        &mut branch,
        ALICE,
        create_record(ns, "a", fixture_cells("book", 10)),
    );
    assert_eq!(recreated.effect, Effect::RecordCreated { id: rid(6) });
    model.create("a", &fixture_cells("book", 10));
    let deleted = apply(&mut branch, ALICE, delete_record(ns, "d"));
    assert_eq!(deleted.effect, Effect::RecordDeleted { id: rid(5) });
    model.delete("d");
    let recreated = apply(
        &mut branch,
        ALICE,
        create_record(ns, "d", fixture_cells("pen", 1)),
    );
    assert_eq!(recreated.effect, Effect::RecordCreated { id: rid(7) });
    model.create("d", &fixture_cells("pen", 1));
    assert_matches_model(&branch, ns, &model, &["a", "b", "c", "d", "e"]);
    let ids: Vec<RecordId> = branch
        .query(
            ns,
            &Query::match_all(),
            &Projection::ID_ONLY,
            10,
            None,
            Budget::unlimited(),
        )
        .expect("reads")
        .value
        .rows
        .iter()
        .map(|row| row.id)
        .collect();
    assert_eq!(ids, vec![rid(2), rid(3), rid(6), rid(7)]);
}

#[test]
fn rollback_releases_ids_and_restores_rows_keys_and_terms_together() {
    let (engine, ns, model) = fixture_abc();
    let mut branch = begin(&engine, None);
    let checkpoint = branch.checkpoint();

    // A transaction: delete a (106), retype b's price to U64 (1 + 3 + {10 +
    // 4 + 13 + RowByte 47 + RootUpdate 2*2} = 82, two index removes at 10 ->
    // 102, two new terms at 40 -> 182), create d with one cell kind=pen (1 +
    // 3 + {20 + 4 + 7 + RowByte (1 + 7) + RootUpdate 3*2} = 49, kind=pen and
    // the Str existence term exist through c: {6 + 4} each -> 59, 69).
    let mut changed = model.clone();
    apply(&mut branch, ALICE, delete_record(ns, "a"));
    changed.delete("a");
    let retype = vec![(name("price"), attr(Value::U64(20)))];
    let patched = apply(
        &mut branch,
        ALICE,
        patch_record(ns, "b", retype.clone(), vec![]),
    );
    assert_eq!(patched.receipt, applied(182));
    changed.patch("b", &retype, &[]);
    let pen = vec![(name("kind"), attr(Value::str("pen")))];
    let created = apply(&mut branch, ALICE, create_record(ns, "d", pen.clone()));
    assert_eq!(created.effect, Effect::RecordCreated { id: rid(4) });
    assert_eq!(created.receipt, applied(69));
    changed.create("d", &pen);
    let keys = ["a", "b", "c", "d", "e", "f"];
    assert_matches_model(&branch, ns, &changed, &keys);

    assert_eq!(branch.rollback_to(checkpoint), Ok(0..3));
    assert_matches_model(&branch, ns, &model, &keys);
    assert_eq!(branch.term_count(ns), Some(ABC_TERMS));
    // Receipts and the block's spent units survive the rollback.
    assert_eq!(branch.receipts().len(), 3);
    assert_eq!(branch.spent(), Cost::new(106 + 182 + 69));

    // The terms are restored, not only the rows: price=10 is held again (by
    // the restored a), so a record holding only it creates no term: 1 + 3 +
    // {20 + 4 + 13 + RowByte (1 + 13) + RootUpdate 3*2} = 61, then {IndexInsert
    // 6 + RootUpdate 4} twice -> 81. And the released id 4 is handed out
    // again.
    let original = model.clone();
    let mut model = model;
    let ten = vec![(name("price"), attr(Value::I64(10)))];
    let created = apply(&mut branch, ALICE, create_record(ns, "f", ten.clone()));
    assert_eq!(created.effect, Effect::RecordCreated { id: rid(4) });
    assert_eq!(created.receipt, applied(81));
    model.create("f", &ten);
    let created = apply(&mut branch, ALICE, create_record(ns, "d", pen.clone()));
    assert_eq!(created.effect, Effect::RecordCreated { id: rid(5) });
    assert_eq!(created.receipt, applied(69));
    model.create("d", &pen);
    assert_matches_model(&branch, ns, &model, &keys);

    // Nested: an inner checkpoint releases only its own id, and a rejected
    // op after it is inside the discarded receipt range.
    let inner = branch.checkpoint();
    let created = apply(&mut branch, ALICE, create_record(ns, "e", pen.clone()));
    assert_eq!(created.effect, Effect::RecordCreated { id: rid(6) });
    let bound = reject(&mut branch, ALICE, create_record(ns, "a", pen.clone()));
    assert_eq!(bound.reason, Rejection::AlreadyExists);
    assert_eq!(branch.rollback_to(inner), Ok(5..7));
    assert_matches_model(&branch, ns, &model, &keys);
    let created = apply(&mut branch, ALICE, create_record(ns, "e", pen.clone()));
    assert_eq!(created.effect, Effect::RecordCreated { id: rid(6) });
    model.create("e", &pen);
    assert_matches_model(&branch, ns, &model, &keys);
    // Rolling back to the outer checkpoint again discards everything since.
    assert_eq!(branch.rollback_to(checkpoint), Ok(0..8));
    assert_matches_model(&branch, ns, &original, &keys);
    assert_eq!(branch.receipts().len(), 8);
}

#[test]
fn a_transfer_shares_the_typed_state_and_visits_no_record_or_index_node() {
    let (mut engine, ns, model) = fixture_abc();
    let before = engine.head_snapshot();
    let mut branch = begin(&engine, None);
    let moved = apply(&mut branch, ALICE, transfer(ns, 0, BOB));
    assert_eq!(
        moved.effect,
        Effect::Transferred {
            revision: Revision(1)
        }
    );
    // DirectoryLookup 1 + NamespaceTransfer 20 + RootUpdate 1*2.
    assert_eq!(
        moved.receipt,
        applied(DIRECTORY_LOOKUP + NAMESPACE_TRANSFER + ROOT_UPDATE)
    );
    assert_eq!(
        moved.physical.record_nodes, 0,
        "no key or row map node is visited"
    );
    assert_eq!(
        moved.physical.index_nodes, 0,
        "no term or posting node is visited"
    );
    assert_matches_model(&branch, ns, &model, &ABC_KEYS);
    publish(&mut engine, branch);
    let after = engine.head_snapshot();

    assert!(after.typed_shared_with(&before, ns));
    assert!(before.typed_shared_with(&after, ns));
    assert!(after.records_shared_with(&before, ns));
    assert!(after.typed_shared_with(&after, ns));
    assert!(
        !after.typed_shared_with(&before, vocab::ns(2)),
        "unknown namespaces share nothing"
    );
    assert_matches_model(&after, ns, &model, &ABC_KEYS);

    // The new owner writes and the old owner is refused after the directory
    // charge; the write copies a path, so the states no longer share.
    let mut changed = model.clone();
    let mut branch = begin(&engine, None);
    let refused = reject(&mut branch, ALICE, delete_record(ns, "a"));
    assert_eq!(refused.receipt, rejected(1, Rejection::Unauthorized));
    let deleted = apply(&mut branch, BOB, delete_record(ns, "a"));
    assert_eq!(deleted.receipt, applied(106));
    changed.delete("a");
    publish(&mut engine, branch);
    let later = engine.head_snapshot();
    assert!(!later.typed_shared_with(&after, ns));
    assert_matches_model(&later, ns, &changed, &ABC_KEYS);
    assert_matches_model(&after, ns, &model, &ABC_KEYS);
}

#[test]
fn raw_bytes_and_typed_records_share_a_namespace_but_never_a_key() {
    let (engine, ns) = engine_with_namespace(Limits::DEFAULT);
    let mut model = Model::new();
    let mut branch = begin(&engine, None);
    let record = key("a");
    // Even the typed key's own encoding (zero-extended to the 20-byte raw
    // key cap), used as a raw key, cannot reach typed data.
    let mut encoded = arkiv_db::encode(&record);
    encoded.resize(20, 0);
    let cells = vec![(name("kind"), attr(Value::str("book")))];
    // create: 1 + 3 + {20 + 4 + (4 + 4) + RowByte (1 + 8) + RootUpdate 3*2}
    // + two new terms * (36 + RootUpdate 2*2) = 4 + 47 + 80 = 131.
    const CREATE: u64 = 131;
    // get_record ALL: 1 + 3 + {3 + (1 + 8)} = 16; match-all page of one row:
    // 1 + QueryGroup 5 + RowVisit 1 + {3 + 9} = 19; of no row: 1 + 5 = 6.
    const GET_RECORD: u64 = 16;
    const PAGE_ONE: u64 = 19;
    const PAGE_NONE: u64 = 6;
    // raw put "a" (1 byte) = "raw": 1 + 10 + 2 + 3 + RootUpdate 2*2 = 20;
    // put of the 20-byte prefix = "enc": 1 + 10 + 40 + 3 + 4 = 58; raw
    // deletes 1 + 5 + 2 + 4 = 12 and 1 + 5 + 40 + 4 = 50; get present
    // 1 + 3 + 3 = 7, missing 4; scan of both entries 1 + 4 + (2 + 20 + 3) +
    // (2 + 1 + 3) = 36.
    const PUT_SHORT: u64 = 20;
    const PUT_ENCODED: u64 = 58;
    const DELETE_SHORT: u64 = 12;
    const DELETE_ENCODED: u64 = 50;
    const GET_HIT: u64 = 7;
    const GET_MISS: u64 = 4;
    const SCAN_BOTH: u64 = 36;

    // A raw entry under the key bytes does not bind the record key.
    let executed = apply(&mut branch, ALICE, put(ns, b"a", b"raw"));
    assert_eq!(executed.effect, Effect::Put { replaced: false });
    assert_eq!(executed.receipt, applied(PUT_SHORT));
    let executed = apply(&mut branch, ALICE, create_record(ns, "a", cells.clone()));
    assert_eq!(executed.effect, Effect::RecordCreated { id: rid(1) });
    assert_eq!(executed.receipt, applied(CREATE));
    model.create("a", &cells);
    let executed = apply(&mut branch, ALICE, put(ns, &encoded, b"enc"));
    assert_eq!(executed.effect, Effect::Put { replaced: false });
    assert_eq!(executed.receipt, applied(PUT_ENCODED));

    let get =
        |branch: &Branch, key: &[u8]| branch.get(ns, key, Budget::unlimited()).expect("reads");
    let read = get(&branch, b"a");
    assert_eq!(
        (read.value, read.receipt),
        (Some(b"raw".to_vec()), read_applied(GET_HIT))
    );
    let read = get(&branch, &encoded);
    assert_eq!(
        (read.value, read.receipt),
        (Some(b"enc".to_vec()), read_applied(GET_HIT))
    );
    let scan = branch
        .scan_prefix(ns, b"", Budget::unlimited())
        .expect("scans");
    assert_eq!(
        scan.value,
        vec![entry(&encoded, b"enc"), entry(b"a", b"raw")]
    );
    assert_eq!(scan.receipt, read_applied(SCAN_BOTH));
    let read = branch
        .get_record(ns, &record, &Projection::ALL, Budget::unlimited())
        .expect("reads");
    assert_eq!(read.value, model.row("a", &Projection::ALL));
    assert_eq!(read.receipt, read_applied(GET_RECORD));
    let page = branch
        .query(
            ns,
            &Query::match_all(),
            &Projection::ALL,
            10,
            None,
            Budget::unlimited(),
        )
        .expect("reads");
    assert_eq!(
        page.value.rows,
        model.evaluate(&Query::match_all(), &Projection::ALL)
    );
    assert!(page.value.next.is_none());
    assert_eq!(page.receipt, read_applied(PAGE_ONE));
    assert_eq!(branch.record_count(ns), Some(2));
    assert_matches_model(&branch, ns, &model, &["a"]);
    assert_eq!(branch.term_count(ns), Some(2));

    // Raw deletes of both spellings leave the typed record alone.
    let executed = apply(&mut branch, ALICE, delete(ns, b"a"));
    assert_eq!(executed.effect, Effect::Deleted { existed: true });
    assert_eq!(executed.receipt, applied(DELETE_SHORT));
    let read = get(&branch, b"a");
    assert_eq!((read.value, read.receipt), (None, read_applied(GET_MISS)));
    assert_matches_model(&branch, ns, &model, &["a"]);
    let executed = apply(&mut branch, ALICE, delete(ns, &encoded));
    assert_eq!(executed.effect, Effect::Deleted { existed: true });
    assert_eq!(executed.receipt, applied(DELETE_ENCODED));
    assert_eq!(branch.record_count(ns), Some(0));
    assert_matches_model(&branch, ns, &model, &["a"]);
    let executed = apply(&mut branch, ALICE, delete(ns, b"a"));
    assert_eq!(
        executed.effect,
        Effect::Deleted { existed: false },
        "the record is not a raw entry"
    );
    assert_eq!(executed.receipt, applied(DELETE_SHORT));

    // A record delete leaves raw entries alone.
    apply(&mut branch, ALICE, put(ns, b"a", b"raw"));
    apply(&mut branch, ALICE, put(ns, &encoded, b"enc"));
    assert_eq!(branch.record_count(ns), Some(2));
    // delete: 1 + 3 + {20 + 4 + RootUpdate 3*2} + {IndexRemove 6 + RootUpdate
    // 2*2} * 2 = 4 + 30 + 20 = 54.
    let executed = apply(&mut branch, ALICE, delete_record(ns, "a"));
    assert_eq!(executed.effect, Effect::RecordDeleted { id: rid(1) });
    assert_eq!(executed.receipt, applied(54));
    model.delete("a");
    assert_matches_model(&branch, ns, &model, &["a"]);
    assert_eq!(branch.record_count(ns), Some(2));
    let read = get(&branch, b"a");
    assert_eq!(
        (read.value, read.receipt),
        (Some(b"raw".to_vec()), read_applied(GET_HIT))
    );
    let read = get(&branch, &encoded);
    assert_eq!(
        (read.value, read.receipt),
        (Some(b"enc".to_vec()), read_applied(GET_HIT))
    );
    let read = branch
        .get_record(ns, &record, &Projection::ALL, Budget::unlimited())
        .expect("reads");
    assert_eq!((read.value, read.receipt), (None, read_applied(GET_MISS)));
    let page = branch
        .query(
            ns,
            &Query::match_all(),
            &Projection::ALL,
            10,
            None,
            Budget::unlimited(),
        )
        .expect("reads");
    assert!(page.value.rows.is_empty() && page.value.next.is_none());
    assert_eq!(page.receipt, read_applied(PAGE_NONE));

    // Recreating the record mints a new id and recreates its terms.
    let executed = apply(&mut branch, ALICE, create_record(ns, "a", cells.clone()));
    assert_eq!(executed.effect, Effect::RecordCreated { id: rid(2) });
    assert_eq!(executed.receipt, applied(CREATE));
    model.create("a", &cells);
    assert_matches_model(&branch, ns, &model, &["a"]);
    assert_eq!(branch.record_count(ns), Some(2));
}

#[test]
fn get_record_refuses_more_than_64_projected_names_before_any_charge() {
    let (engine, ns, model) = fixture_abc();
    let snapshot = engine.head_snapshot();
    let branch = begin(&engine, None);
    let a = key("a");
    // ProjectionTooLarge: 65 names are refused before any charge, even for an
    // unknown namespace; 64 absent names read for the fixed 7.
    let too_many = Projection::named(false, (0..65).map(|i| name(&format!("n{i:02}"))));
    let refused = read_admission(Rejection::InvalidQuery(QueryFault::ProjectionTooLarge));
    for budget in [Budget::unlimited(), Budget::limit(0)] {
        assert_eq!(
            snapshot.get_record(ns, &a, &too_many, budget),
            Err(refused.clone())
        );
        assert_eq!(
            snapshot.get_record(vocab::ns(2), &a, &too_many, budget),
            Err(refused.clone())
        );
        assert_eq!(
            branch.get_record(ns, &a, &too_many, budget),
            Err(refused.clone())
        );
    }
    let at_limit = Projection::named(false, (0..64).map(|i| name(&format!("n{i:02}"))));
    let read = snapshot
        .get_record(ns, &a, &at_limit, Budget::unlimited())
        .expect("64 names are admitted");
    assert_eq!(read.value, model.row("a", &at_limit));
    assert_eq!(read.receipt, read_applied(7));
}

#[test]
fn a_pinned_snapshot_answers_get_record_after_later_patches_and_deletes() {
    let (mut engine, ns, model2) = fixture_abc();
    let s2 = engine.head_snapshot();
    let s1 = engine
        .snapshot(s2.base().expect("snapshot 2 has a base"))
        .expect("snapshot 1 is retained");
    let genesis = engine.snapshot(SnapshotId::GENESIS).expect("retained");
    let keys = ["a", "b", "c"];

    // Block 3: patch a (the worked vector, 183) and delete b (106).
    let mut model3 = model2.clone();
    let mut branch = begin(&engine, None);
    let retype = vec![(name("price"), attr(Value::U64(10)))];
    let remove = vec![name("note"), name("missing")];
    let patched = apply(
        &mut branch,
        ALICE,
        patch_record(ns, "a", retype.clone(), remove.clone()),
    );
    assert_eq!(patched.receipt, applied(183));
    model3.patch("a", &retype, &remove);
    let deleted = apply(&mut branch, ALICE, delete_record(ns, "b"));
    assert_eq!(deleted.receipt, applied(106));
    model3.delete("b");
    publish(&mut engine, branch);
    let s3 = engine.head_snapshot();

    // Block 4: delete a (now 3 cells, 3 attributes: 4 + 20 + 12 + RootUpdate
    // 3*2 + 6*(6 + 4) = 102) and recreate b (id 4): only c is live, so
    // kind=book and price=20 are new: 4 + 20 + 16 + 46 + RowByte 47 +
    // RootUpdate 3*2 + 6*(6 + 4) + 2*30 = 259.
    let mut model4 = model3.clone();
    let mut branch = begin(&engine, None);
    let deleted = apply(&mut branch, ALICE, delete_record(ns, "a"));
    assert_eq!(deleted.effect, Effect::RecordDeleted { id: rid(1) });
    assert_eq!(deleted.receipt, applied(102));
    model4.delete("a");
    let created = apply(
        &mut branch,
        ALICE,
        create_record(ns, "b", fixture_cells("book", 20)),
    );
    assert_eq!(created.effect, Effect::RecordCreated { id: rid(4) });
    assert_eq!(created.receipt, applied(259));
    assert_eq!(model4.create("b", &fixture_cells("book", 20)), rid(4));
    publish(&mut engine, branch);
    let s4 = engine.head_snapshot();

    // Every snapshot answers from its own state with its own charges.
    assert_eq!(genesis.typed_record_count(ns), None);
    let unknown = genesis
        .get_record(ns, &key("a"), &Projection::ALL, Budget::unlimited())
        .expect_err("no namespace at genesis");
    assert_eq!(
        unknown.receipt,
        read_rejected(1, Rejection::UnknownNamespace)
    );
    assert_matches_model(&s1, ns, &Model::new(), &keys);
    assert_eq!(s1.term_count(ns), Some(0));

    assert_matches_model(&s2, ns, &model2, &keys);
    let read = s2
        .get_record(ns, &key("a"), &Projection::ALL, Budget::unlimited())
        .expect("reads");
    assert_eq!(read.receipt, read_applied(54));
    assert_eq!(read.value.map(|row| row.id), Some(rid(1)));

    assert_matches_model(&s3, ns, &model3, &keys);
    // a at snapshot 3: key 1 + $expiresAt 18 + kind 8 + price (5 + 8) = 40
    // projected bytes: 1 + 3 + 3 + 40 = 47.
    let read = s3
        .get_record(ns, &key("a"), &Projection::ALL, Budget::unlimited())
        .expect("reads");
    assert_eq!(read.receipt, read_applied(47));
    assert_eq!(read.value, model3.row("a", &Projection::ALL));
    let read = s3
        .get_record(ns, &key("b"), &Projection::ALL, Budget::unlimited())
        .expect("reads");
    assert_eq!((read.value, read.receipt), (None, read_applied(4)));

    assert_matches_model(&s4, ns, &model4, &keys);
    let read = s4
        .get_record(ns, &key("a"), &Projection::ALL, Budget::unlimited())
        .expect("reads");
    assert_eq!((read.value, read.receipt), (None, read_applied(4)));
    let read = s4
        .get_record(ns, &key("b"), &Projection::ALL, Budget::unlimited())
        .expect("reads");
    assert_eq!(
        read.receipt,
        read_applied(54),
        "the recreated b has the same bytes"
    );
    assert_eq!(read.value.as_ref().map(|row| row.id), Some(rid(4)));
    assert_eq!(read.value, model4.row("b", &Projection::ALL));
    let ids: Vec<RecordId> = s4
        .query(
            ns,
            &Query::match_all(),
            &Projection::ID_ONLY,
            10,
            None,
            Budget::unlimited(),
        )
        .expect("reads")
        .value
        .rows
        .iter()
        .map(|row| row.id)
        .collect();
    assert_eq!(ids, vec![rid(3), rid(4)]);

    // The older snapshots are unchanged by everything published after them,
    // and a branch from head sees the head state.
    assert_matches_model(&s2, ns, &model2, &keys);
    assert_matches_model(&s3, ns, &model3, &keys);
    assert_matches_model(&begin(&engine, None), ns, &model4, &keys);
    assert!(s4.typed_shared_with(&engine.head_snapshot(), ns));
    assert!(!s4.typed_shared_with(&s3, ns));
}

/// A term created inside a rolled-back transaction is new again afterwards.
///
/// Retyping b's price to U64 creates the (price, U64, 20) value term and the
/// (price, U64) existence term, costing 1 + 3 + {10 + 4 + 13 + RowByte 47 +
/// RootUpdate 2×2} + 2 × {IndexRemove 6 + RootUpdate 2×2} + 2 × {6 + 30 +
/// RootUpdate 2×2} = 82 + 20 + 80 = 182. After the rollback a record holding
/// only price U64(20) pays both creations again, 1 + 3 + {20 + 4 + 13 +
/// RowByte (1 + 13) + RootUpdate 3×2} + 2 × {6 + 30 + 4} = 61 + 80 = 141,
/// whereas without the rollback it would pay 61 + 2 × {6 + 4} = 81.
#[test]
fn rollback_makes_terms_created_inside_the_transaction_new_again() {
    let (engine, ns, _model) = fixture_abc();
    let mut branch = begin(&engine, None);
    let retype = vec![(name("price"), attr(Value::U64(20)))];

    // Without a rollback the terms are held by b, so the create pays 81.
    let mut probe = begin(&engine, None);
    let patched = apply(
        &mut probe,
        ALICE,
        patch_record(ns, "b", retype.clone(), vec![]),
    );
    assert_eq!(patched.receipt, applied(182));
    let held = apply(&mut probe, ALICE, create_record(ns, "g", retype.clone()));
    assert_eq!(held.receipt, applied(81));

    let checkpoint = branch.checkpoint();
    let patched = apply(
        &mut branch,
        ALICE,
        patch_record(ns, "b", retype.clone(), vec![]),
    );
    assert_eq!(patched.receipt, applied(182));
    assert_eq!(branch.rollback_to(checkpoint), Ok(0..1));
    let created = apply(&mut branch, ALICE, create_record(ns, "g", retype));
    assert_eq!(created.effect, Effect::RecordCreated { id: rid(4) });
    assert_eq!(created.receipt, applied(141));
    assert_eq!(branch.index_consistent(ns), Some(true));
}
