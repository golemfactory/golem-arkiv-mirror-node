//! Hand-counted metering vectors for the typed record charge points under
//! `PriceSchedule::FIXTURE`: record writes (create, patch, delete), typed
//! reads (`get_record`, `query`, `count`) and the rules around them (abort
//! points, ledgers, block budgets, the host path, estimation, overflow and
//! pinned pricing).
//!
//! Every expected total is written as an explicit sum of literals. To check
//! an operation under every budget, its charge steps are transcribed into
//! the test as atomic groups and priced with the weight table in
//! `support/pricing.rs`; the
//! table's total is asserted equal to the hand-written sum, so nothing here
//! asks the engine or a schedule what an operation costs. `RowByte n` counts
//! the key bytes plus the name and encoded value bytes of every cell of the
//! row after the write. Query results come
//! from a `BTreeMap` model with a full-scan evaluator, never from the
//! engine's index. Argument validation, cursor binding, rollback, expiry and
//! physical traces are covered by other files.

use std::ops::Bound;

use arkiv_db::{
    ArgumentFault, Branch, Budget, Cell, CellName, Cost, Effect, Engine, Limits, Literal, Metered,
    NamespaceId, OperationClass, Outcome, PriceSchedule, Projection, Query, QueryPage, ReadResult,
    Receipt, ReceiptKind, RecordId, Rejection, Row, Term, TypeTag, Value, WriteOp,
};

use crate::support::{
    metering::{
        Expect, Group, ScriptStep, WriteVector, assert_estimate_matches_execute,
        assert_read_vector, assert_write_vector, lookup, point_read, publish_pricing,
    },
    model::Model,
    vocab::{
        self, ALICE, BOB, COST_OVERFLOW, Canonical, Cells, FIXTURE_ID, OUT_OF_BUDGET,
        add_namespace, apply, attr, begin, canonical, create_namespace, create_record,
        delete_record, field, key, ledger, name, names, patch_record, publish, read_receipt,
        receipt, rid, traced, without_ledger, write_receipt,
    },
};

/// `{RecordCreate 1, CellWrite c, CellByte b, RowByte n, RootUpdate 3}`.
fn create_group(cells: u64, bytes: u64, row: u64) -> Group {
    vec![
        (OperationClass::RecordCreate, 1),
        (OperationClass::CellWrite, cells),
        (OperationClass::CellByte, bytes),
        (OperationClass::RowByte, row),
        (OperationClass::RootUpdate, 3),
    ]
}

/// `{RecordPatch 1, CellWrite s, CellByte b, CellRemove r, RowByte n,
/// RootUpdate 2}`.
fn patch_group(set: u64, bytes: u64, remove: u64, row: u64) -> Group {
    vec![
        (OperationClass::RecordPatch, 1),
        (OperationClass::CellWrite, set),
        (OperationClass::CellByte, bytes),
        (OperationClass::CellRemove, remove),
        (OperationClass::RowByte, row),
        (OperationClass::RootUpdate, 2),
    ]
}

/// `{RecordDelete 1, CellRemove c, RootUpdate 3}`.
fn delete_group(cells: u64) -> Group {
    vec![
        (OperationClass::RecordDelete, 1),
        (OperationClass::CellRemove, cells),
        (OperationClass::RootUpdate, 3),
    ]
}

/// `{IndexInsert 1, IndexTermCreate 1 if new, RootUpdate 2}`.
fn insert(new: bool) -> Group {
    vec![
        (OperationClass::IndexInsert, 1),
        (OperationClass::IndexTermCreate, u64::from(new)),
        (OperationClass::RootUpdate, 2),
    ]
}

/// `{IndexRemove 1, RootUpdate 2}`.
fn index_remove() -> Group {
    vec![
        (OperationClass::IndexRemove, 1),
        (OperationClass::RootUpdate, 2),
    ]
}

/// `{ResultRow 1, ReadByte p}`.
fn emit(bytes: u64) -> Group {
    vec![
        (OperationClass::ResultRow, 1),
        (OperationClass::ReadByte, bytes),
    ]
}

fn query_group() -> Group {
    vec![(OperationClass::QueryGroup, 1)]
}

fn term_seek() -> Group {
    vec![(OperationClass::TermSeek, 1)]
}

fn term_visit() -> Group {
    vec![(OperationClass::TermVisit, 1)]
}

fn posting() -> Group {
    vec![(OperationClass::PostingEntry, 1)]
}

fn row_visit() -> Group {
    vec![(OperationClass::RowVisit, 1)]
}

fn probe() -> Group {
    vec![(OperationClass::RecordProbe, 1)]
}

fn cell(text: &str, cell: Cell) -> (CellName, Cell) {
    (name(text), cell)
}

/// The fixture record shape: attributes `kind: Str`, `price: I64`,
/// `$expiresAt: U64(5)` and a field `note: Bytes("xyz")`. Name plus encoded
/// value bytes: `$expiresAt` 10 + 8 = 18, `kind` 4 + len, `note` 4 + 3 = 7,
/// `price` 5 + 8 = 13.
fn fixture_cells(kind: &str, price: i64) -> Cells {
    vec![
        cell("kind", attr(Value::str(kind))),
        cell("price", attr(Value::I64(price))),
        cell("$expiresAt", attr(Value::U64(5))),
        cell("note", field(Value::bytes(b"xyz"))),
    ]
}

/// The fixture records in creation order: ids 1, 2, 3.
const FIXTURE_RECORDS: [(&str, &str, i64); 3] =
    [("a", "book", 10), ("b", "book", 20), ("c", "pen", 5)];

fn eq(text: &str, value: Value) -> Term {
    Term::eq(name(text), value)
}

fn not_eq(text: &str, value: Value) -> Term {
    Term::not_eq(name(text), value)
}

fn range(text: &str, lower: Bound<Value>, upper: Bound<Value>) -> Term {
    Term::range(name(text), lower, upper)
}

fn starts_with(text: &str, prefix: &str) -> Term {
    Term::starts_with(name(text), prefix)
}

fn exists(text: &str, tag: TypeTag) -> Term {
    Term::exists(name(text), tag)
}

fn named(key: bool, names: &[&str]) -> Projection {
    Projection::named(key, names.iter().map(|text| name(text)))
}

/// The first `limit` matches with id > `after`, and the id to continue from
/// iff a further match exists.
fn model_page(
    model: &Model,
    query: &Query,
    projection: &Projection,
    limit: usize,
    after: Option<RecordId>,
) -> PageSummary {
    let mut rows = model.evaluate(query, projection);
    rows.retain(|row| after.is_none_or(|after| row.id > after));
    let further = rows.len() > limit;
    rows.truncate(limit);
    let next = further.then(|| rows.last().expect("limit > 0").id);
    (rows, next)
}

fn model_count(model: &Model, query: &Query) -> u64 {
    u64::try_from(model.ids(query).len()).expect("counts fit u64")
}

/// Rows and the continuation id of a page; the cursor itself is opaque.
type PageSummary = (Vec<Row>, Option<RecordId>);

fn summarize(page: Metered<QueryPage>) -> Metered<PageSummary> {
    let after = page.value.next.as_ref().map(|cursor| cursor.after());
    Metered {
        value: (page.value.rows, after),
        receipt: page.receipt,
        physical: page.physical,
    }
}

fn ids_of(rows: &[Row]) -> Vec<u64> {
    rows.iter().map(|row| row.id.ordinal()).collect()
}

/// The ids of the first page of `query` on `branch`.
fn matching(branch: &Branch, ns: NamespaceId, query: &Query) -> Vec<u64> {
    let page = branch
        .query(
            ns,
            query,
            &Projection::ID_ONLY,
            10,
            None,
            Budget::unlimited(),
        )
        .expect("applies");
    ids_of(&page.value.rows)
}

/// A published namespace (ordinal 1, owned by ALICE) with no records, under
/// `schedule` and `limits`.
fn bare_with(schedule: PriceSchedule, limits: Limits) -> (Engine, NamespaceId) {
    let mut engine = Engine::with_limits(schedule, limits);
    let mut branch = begin(&engine, None);
    let ns = add_namespace(&mut branch, ALICE, "test");
    assert_eq!(ns, vocab::ns(1));
    publish(&mut engine, branch);
    (engine, ns)
}

fn bare() -> (Engine, NamespaceId) {
    bare_with(PriceSchedule::FIXTURE, Limits::DEFAULT)
}

/// The fixture: snapshot 1 holds namespace 1 (ALICE) with records
/// a (id 1), b (id 2), c (id 3) of [`fixture_cells`] shape, kinds book, book,
/// pen and prices 10, 20, 5. The model mirrors it.
fn fixture_with(limits: Limits) -> (Engine, NamespaceId, Model) {
    let mut engine = Engine::with_limits(PriceSchedule::FIXTURE, limits);
    let mut model = Model::new();
    let mut branch = begin(&engine, None);
    let ns = add_namespace(&mut branch, ALICE, "test");
    // The worked vectors: 379 (six new terms), 229
    // (price 20 new), 257 (kind pen and price 5 new).
    for ((key_text, kind, price), expected) in FIXTURE_RECORDS.into_iter().zip([379, 229, 257]) {
        let cells = fixture_cells(kind, price);
        let executed = apply(
            &mut branch,
            ALICE,
            create_record(ns, key_text, cells.clone()),
        );
        assert_eq!(executed.receipt, write_receipt(expected, Outcome::Applied));
        let minted = model.create(key_text, &cells);
        assert_eq!(executed.effect, Effect::RecordCreated { id: minted });
    }
    let published = publish(&mut engine, branch);
    assert_eq!(published.sequence(), 1, "the fixture is snapshot 1");
    (engine, ns, model)
}

fn fixture() -> (Engine, NamespaceId, Model) {
    fixture_with(Limits::DEFAULT)
}

/// A key of the longest allowed length, 32 bytes (the width of an SDK
/// entity key).
const LONG_KEY: &str = "entity-key-with-thirty-two-bytes";

/// Every key any vector in this file touches.
const OBSERVED_KEYS: [&str; 8] = ["a", "b", "c", "d", "e", "f", "z", LONG_KEY];

/// Everything a metered read can observe about the typed store of one
/// namespace, plus its unmetered statistics and index consistency.
#[derive(Debug, PartialEq, Eq)]
struct Observed {
    rows: Vec<Option<Row>>,
    records: Option<usize>,
    terms: Option<usize>,
    consistent: Option<bool>,
}

fn observe(branch: &Branch, ns: NamespaceId) -> Observed {
    Observed {
        rows: OBSERVED_KEYS
            .iter()
            .map(|text| {
                branch
                    .get_record(ns, &key(text), &Projection::ALL, Budget::unlimited())
                    .expect("the namespace exists")
                    .value
            })
            .collect(),
        records: branch.typed_record_count(ns),
        terms: branch.term_count(ns),
        consistent: branch.index_consistent(ns),
    }
}

/// What the model says the observation must be.
fn modelled(model: &Model) -> Observed {
    Observed {
        rows: OBSERVED_KEYS
            .iter()
            .map(|text| model.row(text, &Projection::ALL))
            .collect(),
        records: Some(model.len()),
        terms: Some(model.term_count()),
        consistent: Some(true),
    }
}

/// Sweeps a typed write; see [`assert_write_vector`].
fn assert_record_write(engine: &Engine, ns: NamespaceId, vector: &WriteVector<'_>) -> Branch {
    assert_write_vector(engine, vector, |branch| observe(branch, ns))
}

/// A query vector on one reader: the page is checked against the model and
/// against the hand-listed ids and continuation.
struct QueryVector<'a> {
    name: &'a str,
    query: Query,
    projection: Projection,
    limit: usize,
    after: Option<RecordId>,
    groups: Vec<Group>,
    total: u64,
    ids: &'a [u64],
    next: Option<u64>,
}

impl<'a> QueryVector<'a> {
    /// The first page of up to 10 rows, with no further match.
    fn new(
        name: &'a str,
        query: Query,
        projection: Projection,
        groups: Vec<Group>,
        total: u64,
        ids: &'a [u64],
    ) -> Self {
        Self {
            name,
            query,
            projection,
            limit: 10,
            after: None,
            groups,
            total,
            ids,
            next: None,
        }
    }

    fn limit(self, limit: usize) -> Self {
        Self { limit, ..self }
    }

    /// The page continues a cursor positioned after `id`.
    fn after(self, id: RecordId) -> Self {
        Self {
            after: Some(id),
            ..self
        }
    }

    /// The page is full and a further match exists after `ordinal`.
    fn next(self, ordinal: u64) -> Self {
        Self {
            next: Some(ordinal),
            ..self
        }
    }
}

/// A query whose term nobody holds: the lookup, the group and the seek, no
/// rows.
fn assert_seek_only(
    engine: &Engine,
    ns: NamespaceId,
    model: &Model,
    name: &str,
    query: Query,
    projection: Projection,
) {
    assert_query_on_both(
        engine,
        ns,
        model,
        &QueryVector::new(
            name,
            query,
            projection,
            vec![lookup(), query_group(), term_seek()],
            1 + 5 + 4,
            &[],
        ),
    );
}

fn assert_query_vector(
    model: &Model,
    vector: &QueryVector<'_>,
    read: impl Fn(Budget) -> ReadResult<QueryPage>,
) {
    let expected = model_page(
        model,
        &vector.query,
        &vector.projection,
        vector.limit,
        vector.after,
    );
    assert_eq!(
        ids_of(&expected.0),
        vector.ids,
        "{}: model ids",
        vector.name
    );
    assert_eq!(
        expected.1.map(RecordId::ordinal),
        vector.next,
        "{}: model continuation",
        vector.name
    );
    assert_read_vector(
        vector.name,
        &vector.groups,
        vector.total,
        &expected,
        |budget| read(budget).map(summarize),
    );
}

#[test]
fn create_vectors_on_an_empty_namespace_match_the_worked_examples() {
    let (engine, ns) = bare();
    let a = create_record(ns, "a", fixture_cells("book", 10));
    let b = create_record(ns, "b", fixture_cells("book", 20));
    let c = create_record(ns, "c", fixture_cells("pen", 5));

    // create a: DirectoryLookup 1 -> 1; PointRead 3 -> 4; {RecordCreate 20 +
    // CellWrite 4*4 + CellByte 46 + RowByte 47 (key 1 + 46) + RootUpdate
    // 3*2} -> 139; then per attribute in name order ($expiresAt, kind, price)
    // a value term and an existence term, all six new: {IndexInsert 6 +
    // IndexTermCreate 30 + RootUpdate 2*2} = 40 each -> 179, 219, 259, 299,
    // 339, 379.
    assert_record_write(
        &engine,
        ns,
        &WriteVector::new(
            "create a",
            a.clone(),
            vec![
                lookup(),
                point_read(),
                create_group(4, 46, 47),
                insert(true),
                insert(true),
                insert(true),
                insert(true),
                insert(true),
                insert(true),
            ],
            1 + 3 + (20 + 16 + 46 + 47 + 6) + 40 + 40 + 40 + 40 + 40 + 40,
            Expect::Applied(Effect::RecordCreated { id: rid(1) }),
        ),
    );

    // create b after a: same bytes (book is 4 bytes); only the price value
    // term (20) is new, every other insert is {IndexInsert 6 + RootUpdate
    // 2*2} = 10: 139 + 10 + 10 + 10 + 10 + 40 + 10 = 229.
    assert_record_write(
        &engine,
        ns,
        &WriteVector::new(
            "create b",
            b.clone(),
            vec![
                lookup(),
                point_read(),
                create_group(4, 46, 47),
                insert(false),
                insert(false),
                insert(false),
                insert(false),
                insert(true),
                insert(false),
            ],
            1 + 3 + (20 + 16 + 46 + 47 + 6) + 10 + 10 + 10 + 10 + 40 + 10,
            Expect::Applied(Effect::RecordCreated { id: rid(2) }),
        )
        .setup(std::slice::from_ref(&a)),
    );

    // create c after a and b: kind pen is 3 bytes (b = 45, n = 46); kind pen
    // and price 5 are new value terms: 1 + 3 + 133 + 10 + 10 + 40 + 10 + 40 +
    // 10 = 257.
    let branch = assert_record_write(
        &engine,
        ns,
        &WriteVector::new(
            "create c",
            c,
            vec![
                lookup(),
                point_read(),
                create_group(4, 45, 46),
                insert(false),
                insert(false),
                insert(true),
                insert(false),
                insert(true),
                insert(false),
            ],
            1 + 3 + (20 + 16 + 45 + 46 + 6) + 10 + 10 + 40 + 10 + 40 + 10,
            Expect::Applied(Effect::RecordCreated { id: rid(3) }),
        )
        .setup(&[a.clone(), b]),
    );

    // The applied state is what the model says: 3 records, 9 terms (three
    // existence terms plus value terms $expiresAt 5, kind book/pen, price
    // 5/10/20).
    let mut model = Model::new();
    for (key_text, kind, price) in FIXTURE_RECORDS {
        model.create(key_text, &fixture_cells(kind, price));
    }
    assert_eq!(model.term_count(), 9);
    assert_eq!(observe(&branch, ns), modelled(&model));

    // The traced ledger of create a, spelled out.
    let mut branch = begin(&engine, None);
    let executed = branch
        .execute(ALICE, a, Budget::limit(379).traced())
        .expect("an exact budget succeeds");
    assert_eq!(
        executed.receipt,
        traced(
            write_receipt(379, Outcome::Applied),
            ledger(&[
                (OperationClass::DirectoryLookup, 1, 1, 1),
                (OperationClass::PointRead, 1, 3, 3),
                (OperationClass::RecordCreate, 1, 20, 20),
                (OperationClass::CellWrite, 4, 4, 16),
                (OperationClass::CellByte, 46, 1, 46),
                (OperationClass::RowByte, 47, 1, 47),
                // First charged in the record group (3), then 2 per insert.
                (OperationClass::RootUpdate, 3 + 6 * 2, 2, 30),
                (OperationClass::IndexInsert, 6, 6, 36),
                (OperationClass::IndexTermCreate, 6, 30, 180),
            ])
        )
    );
}

#[test]
fn create_with_no_new_term_charges_lookup_binding_group_and_six_inserts() {
    let (engine, ns, mut model) = fixture();
    // create d with a's cells: every term exists, so each of the six index
    // charges is {IndexInsert 6 + RootUpdate 2*2} = 10: 1 + 3 + 135 + 60 =
    // 199.
    let cells = fixture_cells("book", 10);
    let branch = assert_record_write(
        &engine,
        ns,
        &WriteVector::new(
            "create d (no new term)",
            create_record(ns, "d", cells.clone()),
            vec![
                lookup(),
                point_read(),
                create_group(4, 46, 47),
                insert(false),
                insert(false),
                insert(false),
                insert(false),
                insert(false),
                insert(false),
            ],
            1 + 3 + (20 + 16 + 46 + 47 + 6) + 10 + 10 + 10 + 10 + 10 + 10,
            Expect::Applied(Effect::RecordCreated { id: rid(4) }),
        ),
    );
    model.create("d", &cells);
    assert_eq!(model.term_count(), 9, "no term was created");
    assert_eq!(observe(&branch, ns), modelled(&model));
    // The ledger has no IndexTermCreate line at all.
    let mut branch = begin(&engine, None);
    let executed = branch
        .execute(
            ALICE,
            create_record(ns, "d", cells),
            Budget::unlimited().traced(),
        )
        .expect("applies");
    assert_eq!(
        executed.receipt,
        traced(
            write_receipt(199, Outcome::Applied),
            ledger(&[
                (OperationClass::DirectoryLookup, 1, 1, 1),
                (OperationClass::PointRead, 1, 3, 3),
                (OperationClass::RecordCreate, 1, 20, 20),
                (OperationClass::CellWrite, 4, 4, 16),
                (OperationClass::CellByte, 46, 1, 46),
                (OperationClass::RowByte, 47, 1, 47),
                (OperationClass::RootUpdate, 15, 2, 30),
                (OperationClass::IndexInsert, 6, 6, 36),
            ])
        )
    );
}

#[test]
fn create_with_some_new_terms_charges_term_creation_only_for_them() {
    let (engine, ns, mut model) = fixture();
    // create d: kind "ink" (attribute, 4 + 3 = 7 bytes; value term new,
    // existence (kind, Str) held), price I64(99) (13 bytes; value new,
    // existence held), note field (7 bytes, unindexed): c = 3, a = 2,
    // b = 27, n = 1 + 27 = 28. 1 + 3 + (20 + 12 + 27 + 28 + 6) + 40 + 10 +
    // 40 + 10 = 197.
    let cells = vec![
        cell("kind", attr(Value::str("ink"))),
        cell("price", attr(Value::I64(99))),
        cell("note", field(Value::bytes(b"xyz"))),
    ];
    let branch = assert_record_write(
        &engine,
        ns,
        &WriteVector::new(
            "create d (some new terms)",
            create_record(ns, "d", cells.clone()),
            vec![
                lookup(),
                point_read(),
                create_group(3, 27, 28),
                insert(true),
                insert(false),
                insert(true),
                insert(false),
            ],
            1 + 3 + (20 + 12 + 27 + 28 + 6) + 40 + 10 + 40 + 10,
            Expect::Applied(Effect::RecordCreated { id: rid(4) }),
        ),
    );
    model.create("d", &cells);
    assert_eq!(model.term_count(), 11, "two value terms were created");
    assert_eq!(observe(&branch, ns), modelled(&model));
}

#[test]
fn create_with_attribute_and_field_mix_indexes_only_the_attributes() {
    let (engine, ns, mut model) = fixture();
    // create e: color attribute Bool(true) (5 + 1 = 6 bytes; both terms
    // new), price *field* I64(10) (13 bytes; no index work although the
    // value term exists), tag field Str("x") (3 + 1 = 4 bytes): c = 3,
    // a = 1, b = 23, n = 24. 1 + 3 + (20 + 12 + 23 + 24 + 6) + 40 + 40 =
    // 169.
    let cells = vec![
        cell("color", attr(Value::Bool(true))),
        cell("price", field(Value::I64(10))),
        cell("tag", field(Value::str("x"))),
    ];
    let branch = assert_record_write(
        &engine,
        ns,
        &WriteVector::new(
            "create e (mixed kinds)",
            create_record(ns, "e", cells.clone()),
            vec![
                lookup(),
                point_read(),
                create_group(3, 23, 24),
                insert(true),
                insert(true),
            ],
            1 + 3 + (20 + 12 + 23 + 24 + 6) + 40 + 40,
            Expect::Applied(Effect::RecordCreated { id: rid(4) }),
        ),
    );
    model.create("e", &cells);
    assert_eq!(model.term_count(), 11);
    assert_eq!(observe(&branch, ns), modelled(&model));
    // The fields are stored and returned but match no term.
    let query = Query::term(eq("price", Value::I64(10)));
    assert_eq!(model.ids(&query), vec![rid(1)]);
    assert_eq!(matching(&branch, ns, &query), vec![1]);
}

#[test]
fn create_with_no_cells_charges_only_the_record_unit() {
    let (engine, ns, mut model) = fixture();
    // create z with no cells: c = 0, a = 0, b = 0, n = key 1: 1 + 3 + (20 +
    // RowByte 1 + RootUpdate 3*2) = 31, and the zero-count classes never
    // reach the ledger.
    let branch = assert_record_write(
        &engine,
        ns,
        &WriteVector::new(
            "create z (no cells)",
            create_record(ns, "z", Vec::new()),
            vec![lookup(), point_read(), create_group(0, 0, 1)],
            1 + 3 + (20 + 1 + 6),
            Expect::Applied(Effect::RecordCreated { id: rid(4) }),
        ),
    );
    model.create("z", &[]);
    assert_eq!(observe(&branch, ns), modelled(&model));
    let mut branch = begin(&engine, None);
    let executed = branch
        .execute(
            ALICE,
            create_record(ns, "z", Vec::new()),
            Budget::limit(31).traced(),
        )
        .expect("31 is exact");
    assert_eq!(
        executed.receipt,
        traced(
            write_receipt(31, Outcome::Applied),
            ledger(&[
                (OperationClass::DirectoryLookup, 1, 1, 1),
                (OperationClass::PointRead, 1, 3, 3),
                (OperationClass::RecordCreate, 1, 20, 20),
                (OperationClass::RowByte, 1, 1, 1),
                (OperationClass::RootUpdate, 3, 2, 6),
            ])
        )
    );
    // get_record z, ALL: p = key 1 + no cells = 1: 1 + 3 + (3 + 1) = 8.
    let row = branch
        .get_record(ns, &key("z"), &Projection::ALL, Budget::unlimited())
        .expect("applies");
    assert_eq!(row.receipt, read_receipt(1 + 3 + (3 + 1), Outcome::Applied));
    assert_eq!(row.value, model.row("z", &Projection::ALL));
}

#[test]
fn create_and_get_record_under_a_32_byte_key_count_the_raw_key_bytes() {
    let (engine, ns, mut model) = fixture();
    assert_eq!(LONG_KEY.len(), 32, "the vector needs the longest key");
    // create the 32-byte key with a's cells: every term exists. The row
    // counts the 32 raw key bytes, not the 33-byte canonical encoding:
    // n = 32 + 46 = 78. 1 + 3 + (20 + 16 + 46 + 78 + 6) + 6 * 10 = 230, the
    // create of d (199) plus 31 more key bytes.
    let cells = fixture_cells("book", 10);
    let branch = assert_record_write(
        &engine,
        ns,
        &WriteVector::new(
            "create 32-byte key",
            create_record(ns, LONG_KEY, cells.clone()),
            vec![
                lookup(),
                point_read(),
                create_group(4, 46, 78),
                insert(false),
                insert(false),
                insert(false),
                insert(false),
                insert(false),
                insert(false),
            ],
            1 + 3 + (20 + 16 + 46 + 78 + 6) + 10 + 10 + 10 + 10 + 10 + 10,
            Expect::Applied(Effect::RecordCreated { id: rid(4) }),
        ),
    );
    model.create(LONG_KEY, &cells);
    assert_eq!(observe(&branch, ns), modelled(&model));
    let mut traced_branch = begin(&engine, None);
    let executed = traced_branch
        .execute(
            ALICE,
            create_record(ns, LONG_KEY, cells),
            Budget::unlimited().traced(),
        )
        .expect("applies");
    assert_eq!(
        executed.receipt,
        traced(
            write_receipt(230, Outcome::Applied),
            ledger(&[
                (OperationClass::DirectoryLookup, 1, 1, 1),
                (OperationClass::PointRead, 1, 3, 3),
                (OperationClass::RecordCreate, 1, 20, 20),
                (OperationClass::CellWrite, 4, 4, 16),
                (OperationClass::CellByte, 46, 1, 46),
                (OperationClass::RowByte, 32 + 46, 1, 78),
                (OperationClass::RootUpdate, 3 + 6 * 2, 2, 30),
                (OperationClass::IndexInsert, 6, 6, 36),
            ])
        )
    );

    // get_record projects the 32 raw key bytes: KEY_ONLY p = 32: 1 + 3 +
    // (3 + 32) = 39; ALL p = 32 + 46 = 78: 1 + 3 + (3 + 78) = 85.
    let vectors: [(&str, Projection, Vec<Group>, u64); 2] = [
        (
            "key only",
            Projection::KEY_ONLY,
            vec![lookup(), point_read(), emit(32)],
            1 + 3 + (3 + 32),
        ),
        (
            "all",
            Projection::ALL,
            vec![lookup(), point_read(), emit(78)],
            1 + 3 + (3 + 78),
        ),
    ];
    for (label, projection, groups, total) in &vectors {
        assert_read_vector(
            &format!("get_record 32-byte key {label}"),
            groups,
            *total,
            &model.row(LONG_KEY, projection),
            |budget| branch.get_record(ns, &key(LONG_KEY), projection, budget),
        );
    }
    let read = branch
        .get_record(
            ns,
            &key(LONG_KEY),
            &Projection::KEY_ONLY,
            Budget::unlimited().traced(),
        )
        .expect("applies");
    assert_eq!(
        read.receipt,
        traced(
            read_receipt(39, Outcome::Applied),
            ledger(&[
                (OperationClass::DirectoryLookup, 1, 1, 1),
                (OperationClass::PointRead, 1, 3, 3),
                (OperationClass::ResultRow, 1, 3, 3),
                (OperationClass::ReadByte, 32, 1, 32),
            ])
        )
    );
    assert_eq!(
        read.value,
        Some(Row {
            id: rid(4),
            key: Some(key(LONG_KEY)),
            cells: Vec::new(),
        })
    );
}

#[test]
fn each_new_term_costs_exactly_thirty_more_than_the_same_create() {
    let (engine, ns, model) = fixture();
    // Every variant keeps the shape of a's cells (c = 4, a = 3, b = 46,
    // n = 47: the renamed names have the same length) and differs only in
    // which terms are new: 199 + 30 per new term (RootUpdate is charged per
    // insert whether or not the term is new). A new value under a held name is one
    // new term; an attribute under a new name is two (value and existence).
    /// (label, new terms, cells).
    type Variant = (&'static str, u64, Cells);
    let note = || cell("note", field(Value::bytes(b"xyz")));
    let variants: Vec<Variant> = vec![
        ("no new term", 0, fixture_cells("book", 10)),
        (
            "kind bolt (1)",
            1,
            vec![
                cell("kind", attr(Value::str("bolt"))),
                cell("price", attr(Value::I64(10))),
                cell("$expiresAt", attr(Value::U64(5))),
                note(),
            ],
        ),
        (
            "kind bolt, price 11 (2)",
            2,
            vec![
                cell("kind", attr(Value::str("bolt"))),
                cell("price", attr(Value::I64(11))),
                cell("$expiresAt", attr(Value::U64(5))),
                note(),
            ],
        ),
        (
            "kind bolt, price 11, $expiresAt 6 (3)",
            3,
            vec![
                cell("kind", attr(Value::str("bolt"))),
                cell("price", attr(Value::I64(11))),
                cell("$expiresAt", attr(Value::U64(6))),
                note(),
            ],
        ),
        (
            "kind bolt, price 11, $expiresAu 5 (1 + 1 + 2 = 4)",
            4,
            vec![
                cell("kind", attr(Value::str("bolt"))),
                cell("price", attr(Value::I64(11))),
                cell("$expiresAu", attr(Value::U64(5))),
                note(),
            ],
        ),
        (
            "kinf book, pricf 10, $expiresAt 6 (2 + 2 + 1 = 5)",
            5,
            vec![
                cell("kinf", attr(Value::str("book"))),
                cell("pricf", attr(Value::I64(10))),
                cell("$expiresAt", attr(Value::U64(6))),
                note(),
            ],
        ),
        (
            "kinf book, pricf 10, $expiresAu 5 (2 + 2 + 2 = 6, as on an empty namespace)",
            6,
            vec![
                cell("kinf", attr(Value::str("book"))),
                cell("pricf", attr(Value::I64(10))),
                cell("$expiresAu", attr(Value::U64(5))),
                note(),
            ],
        ),
    ];
    for (label, new_terms, cells) in variants {
        let mut branch = begin(&engine, None);
        let executed = branch
            .execute(
                ALICE,
                create_record(ns, "d", cells.clone()),
                Budget::unlimited(),
            )
            .unwrap_or_else(|rejected| panic!("{label}: {rejected}"));
        assert_eq!(
            executed.receipt,
            write_receipt(199 + 30 * new_terms, Outcome::Applied),
            "{label}"
        );
        let mut expected = model.clone();
        expected.create("d", &cells);
        assert_eq!(
            expected.term_count(),
            9 + usize::try_from(new_terms).expect("small"),
            "{label}: model term count"
        );
        assert_eq!(observe(&branch, ns), modelled(&expected), "{label}");
    }
}

#[test]
fn patch_changing_only_a_value_swaps_the_value_term_and_keeps_existence() {
    let (engine, ns, model) = fixture();
    // patch a: set price := I64(11) (s = 1, b = 13, r = 0, n = 47: the row
    // keeps 46 cell bytes plus key 1): 1; 3 -> 4; {RecordPatch 10 +
    // CellWrite 4 + CellByte 13 + RowByte 47 + RootUpdate 2*2} -> 82;
    // old [value 10, existence I64], new [value 11, existence I64]:
    // {IndexRemove 6 + RootUpdate 4} (value 10) -> 92; {IndexInsert 6 +
    // IndexTermCreate 30 + RootUpdate 4} (value 11 is new) -> 132. The
    // existence term is untouched.
    let set = vec![cell("price", attr(Value::I64(11)))];
    let branch = assert_record_write(
        &engine,
        ns,
        &WriteVector::new(
            "patch a price := 11",
            patch_record(ns, "a", set.clone(), Vec::new()),
            vec![
                lookup(),
                point_read(),
                patch_group(1, 13, 0, 47),
                index_remove(),
                insert(true),
            ],
            1 + 3 + (10 + 4 + 13 + 47 + 4) + 10 + 40,
            Expect::Applied(Effect::RecordPatched { id: rid(1) }),
        ),
    );
    let mut expected = model.clone();
    expected.patch("a", &set, &[]);
    assert_eq!(expected.term_count(), 9, "value 10 dropped, value 11 added");
    assert_eq!(observe(&branch, ns), modelled(&expected));

    // set price := I64(20), a value b already holds: the insert is not a
    // term creation: 82 + 10 + 10 = 102.
    let set = vec![cell("price", attr(Value::I64(20)))];
    let branch = assert_record_write(
        &engine,
        ns,
        &WriteVector::new(
            "patch a price := 20",
            patch_record(ns, "a", set.clone(), Vec::new()),
            vec![
                lookup(),
                point_read(),
                patch_group(1, 13, 0, 47),
                index_remove(),
                insert(false),
            ],
            1 + 3 + (10 + 4 + 13 + 47 + 4) + 10 + 10,
            Expect::Applied(Effect::RecordPatched { id: rid(1) }),
        ),
    );
    let mut expected = model;
    expected.patch("a", &set, &[]);
    assert_eq!(expected.term_count(), 8, "value 10 was the only holder");
    assert_eq!(observe(&branch, ns), modelled(&expected));
}

#[test]
fn patch_changing_a_type_removes_and_creates_both_terms() {
    let (engine, ns, model) = fixture();
    // patch a: set price := U64(10): old [value I64 10, existence I64], new
    // [value U64 10, existence U64], nothing in common: 1 + 3 + 78 = 82
    // (n = 47); {IndexRemove 6 + RootUpdate 4} (value) -> 92; (existence)
    // -> 102; {6 + 30 + 4} (value U64 10 new) -> 142; {6 + 30 + 4}
    // (existence (price, U64) new) -> 182.
    let set = vec![cell("price", attr(Value::U64(10)))];
    let branch = assert_record_write(
        &engine,
        ns,
        &WriteVector::new(
            "patch a price := U64(10)",
            patch_record(ns, "a", set.clone(), Vec::new()),
            vec![
                lookup(),
                point_read(),
                patch_group(1, 13, 0, 47),
                index_remove(),
                index_remove(),
                insert(true),
                insert(true),
            ],
            1 + 3 + (10 + 4 + 13 + 47 + 4) + 10 + 10 + 40 + 40,
            Expect::Applied(Effect::RecordPatched { id: rid(1) }),
        ),
    );
    let mut expected = model;
    expected.patch("a", &set, &[]);
    assert_eq!(expected.term_count(), 10);
    assert_eq!(observe(&branch, ns), modelled(&expected));
    // A later query on the old type no longer sees a; the new type does.
    let query = Query::term(exists("price", TypeTag::I64));
    assert_eq!(expected.ids(&query), vec![rid(2), rid(3)]);
    assert_eq!(matching(&branch, ns, &query), vec![2, 3]);
    let query = Query::term(eq("price", Value::U64(10)));
    assert_eq!(expected.ids(&query), vec![rid(1)]);
    assert_eq!(matching(&branch, ns, &query), vec![1]);
}

#[test]
fn patch_with_removals_charges_absent_names_and_reindexes_in_name_order() {
    let (engine, ns, model) = fixture();
    // The worked vector. patch a: set price := U64(10), remove
    // [note, missing]: s = 1, b = 13, r = 2, n = 1 + 18 + 8 + 13 = 40: 1; 4;
    // {10 + 4 + 13 + 8 + 40 + 4} -> 83; names in sorted order: missing
    // (absent -> nothing), note (field -> nothing), price: two index removes
    // at 10 -> 103; insert value U64 (new) at 40 -> 143; insert existence
    // U64 (new) -> 183.
    let set = vec![cell("price", attr(Value::U64(10)))];
    let branch = assert_record_write(
        &engine,
        ns,
        &WriteVector::new(
            "patch a (worked vector)",
            patch_record(ns, "a", set.clone(), names(&["note", "missing"])),
            vec![
                lookup(),
                point_read(),
                patch_group(1, 13, 2, 40),
                index_remove(),
                index_remove(),
                insert(true),
                insert(true),
            ],
            1 + 3 + (10 + 4 + 13 + 8 + 40 + 4) + 10 + 10 + 40 + 40,
            Expect::Applied(Effect::RecordPatched { id: rid(1) }),
        ),
    );
    let mut expected = model;
    expected.patch("a", &set, &names(&["note", "missing"]));
    assert_eq!(observe(&branch, ns), modelled(&expected));
    let row = branch
        .get_record(ns, &key("a"), &Projection::ALL, Budget::unlimited())
        .expect("applies")
        .value
        .expect("a is live");
    assert_eq!(row.cells.len(), 3, "note was removed");
    // The traced ledger, spelled out.
    let mut branch = begin(&engine, None);
    let executed = branch
        .execute(
            ALICE,
            patch_record(ns, "a", set, names(&["note", "missing"])),
            Budget::limit(183).traced(),
        )
        .expect("183 is exact");
    assert_eq!(
        executed.receipt,
        traced(
            write_receipt(183, Outcome::Applied),
            ledger(&[
                (OperationClass::DirectoryLookup, 1, 1, 1),
                (OperationClass::PointRead, 1, 3, 3),
                (OperationClass::RecordPatch, 1, 10, 10),
                (OperationClass::CellWrite, 1, 4, 4),
                (OperationClass::CellByte, 13, 1, 13),
                (OperationClass::CellRemove, 2, 4, 8),
                (OperationClass::RowByte, 40, 1, 40),
                (OperationClass::RootUpdate, 2 + 4 * 2, 2, 20),
                (OperationClass::IndexRemove, 2, 6, 12),
                (OperationClass::IndexInsert, 2, 6, 12),
                (OperationClass::IndexTermCreate, 2, 30, 60),
            ])
        )
    );
}

#[test]
fn patch_removing_an_attribute_drops_its_value_then_existence_term() {
    let (engine, ns, model) = fixture();
    // patch a: remove [price]: s = 0, b = 0, r = 1, n = 47 - 13 = 34: 1; 4;
    // {10 + 4 + 34 + 4} -> 56; {IndexRemove 6 + RootUpdate 4} (value 10) ->
    // 66; (existence I64) -> 76.
    let branch = assert_record_write(
        &engine,
        ns,
        &WriteVector::new(
            "patch a remove price",
            patch_record(ns, "a", Vec::new(), names(&["price"])),
            vec![
                lookup(),
                point_read(),
                patch_group(0, 0, 1, 34),
                index_remove(),
                index_remove(),
            ],
            1 + 3 + (10 + 4 + 34 + 4) + 10 + 10,
            Expect::Applied(Effect::RecordPatched { id: rid(1) }),
        ),
    );
    let mut expected = model.clone();
    expected.patch("a", &[], &names(&["price"]));
    assert_eq!(
        expected.term_count(),
        8,
        "value 10 is dropped; existence stays"
    );
    assert_eq!(observe(&branch, ns), modelled(&expected));

    // Three remove-only patches of a in a row.
    // remove [missing]: 1 + 3 + {RecordPatch 10 + CellRemove 4 + RowByte 47
    // (the unchanged row) + RootUpdate 2*2} = 69; an absent name is a
    // charged no-op.
    // remove [note] (a field): no index work; the row shrinks to 47 - 7 =
    // 40 bytes: 1 + 3 + {10 + 4 + 40 + 4} = 62.
    // remove [kind] (an attribute): the row shrinks to 40 - 8 = 32 bytes:
    // 1 + 3 + {10 + 4 + 32 + 4} = 54, then {IndexRemove 6 + RootUpdate 4}
    // twice -> 74; kind=book and the Str existence term survive through b.
    let removals = [
        patch_record(ns, "a", Vec::new(), names(&["missing"])),
        patch_record(ns, "a", Vec::new(), names(&["note"])),
        patch_record(ns, "a", Vec::new(), names(&["kind"])),
    ];
    let vectors: [(&str, u64, usize, u64); 3] = [
        ("patch a remove missing", 47, 0, 69),
        ("patch a remove note", 40, 0, 62),
        ("patch a remove kind", 32, 2, 74),
    ];
    let mut expected = model;
    for (step, (name, row, index_removes, total)) in vectors.into_iter().enumerate() {
        let mut groups = vec![lookup(), point_read(), patch_group(0, 0, 1, row)];
        groups.extend(std::iter::repeat_n(index_remove(), index_removes));
        let branch = assert_record_write(
            &engine,
            ns,
            &WriteVector::new(
                name,
                removals[step].clone(),
                groups,
                total,
                Expect::Applied(Effect::RecordPatched { id: rid(1) }),
            )
            .setup(&removals[..step]),
        );
        expected.patch("a", &[], &names(&[["missing", "note", "kind"][step]]));
        assert_eq!(expected.term_count(), 9, "{name}");
        assert_eq!(observe(&branch, ns), modelled(&expected), "{name}");
    }
}

#[test]
fn patch_flipping_kind_in_both_directions_charges_the_index_each_way() {
    let (engine, ns, model) = fixture();
    // attribute -> field: set kind := field "book" (s = 1, b = 8, n = 47):
    // 1; 4; {10 + 4 + 8 + 47 + 4} -> 77; old [value book, existence Str],
    // new []: {IndexRemove 6 + RootUpdate 4} -> 87; -> 97. Both terms stay
    // in the index because b holds them.
    let to_field = patch_record(
        ns,
        "a",
        vec![cell("kind", field(Value::str("book")))],
        Vec::new(),
    );
    let branch = assert_record_write(
        &engine,
        ns,
        &WriteVector::new(
            "patch a kind -> field",
            to_field.clone(),
            vec![
                lookup(),
                point_read(),
                patch_group(1, 8, 0, 47),
                index_remove(),
                index_remove(),
            ],
            1 + 3 + (10 + 4 + 8 + 47 + 4) + 10 + 10,
            Expect::Applied(Effect::RecordPatched { id: rid(1) }),
        ),
    );
    let mut after_flip = model.clone();
    after_flip.patch("a", &[cell("kind", field(Value::str("book")))], &[]);
    assert_eq!(after_flip.term_count(), 9);
    assert_eq!(observe(&branch, ns), modelled(&after_flip));
    let query = Query::term(eq("kind", Value::str("book")));
    assert_eq!(after_flip.ids(&query), vec![rid(2)]);
    assert_eq!(
        matching(&branch, ns, &query),
        vec![2],
        "a field never matches"
    );

    // field -> attribute, on the flipped state: set kind := attribute
    // "book": old [], new [value book, existence Str], both held by b:
    // 77 + 10 + 10 = 97.
    let branch = assert_record_write(
        &engine,
        ns,
        &WriteVector::new(
            "patch a kind -> attribute",
            patch_record(
                ns,
                "a",
                vec![cell("kind", attr(Value::str("book")))],
                Vec::new(),
            ),
            vec![
                lookup(),
                point_read(),
                patch_group(1, 8, 0, 47),
                insert(false),
                insert(false),
            ],
            1 + 3 + (10 + 4 + 8 + 47 + 4) + 10 + 10,
            Expect::Applied(Effect::RecordPatched { id: rid(1) }),
        )
        .setup(std::slice::from_ref(&to_field)),
    );
    assert_eq!(
        observe(&branch, ns),
        modelled(&model),
        "back to the fixture"
    );

    // attribute -> field where the record is the value term's only holder:
    // set kind := field Str("pen") on c: 1 -> 4 -> {RecordPatch 10 +
    // CellWrite 4 + CellByte (4 + 3) + RowByte 46 (c's row keeps its 45 cell
    // bytes plus key 1) + RootUpdate 2*2} = 71 -> 75; {IndexRemove 6 +
    // RootUpdate 4} -> 85, -> 95. kind=pen is dropped; kind's Str existence
    // term stays through a and b: 9 - 1.
    let set = vec![cell("kind", field(Value::str("pen")))];
    let branch = assert_record_write(
        &engine,
        ns,
        &WriteVector::new(
            "patch c kind -> field",
            patch_record(ns, "c", set.clone(), Vec::new()),
            vec![
                lookup(),
                point_read(),
                patch_group(1, 7, 0, 46),
                index_remove(),
                index_remove(),
            ],
            95,
            Expect::Applied(Effect::RecordPatched { id: rid(3) }),
        ),
    );
    let mut expected = model.clone();
    expected.patch("c", &set, &[]);
    assert_eq!(expected.term_count(), 8);
    assert_eq!(observe(&branch, ns), modelled(&expected));

    // A field becoming an attribute of a type nobody holds creates both
    // terms: set note := attribute Str("xyz") (b = 7, n = 47): 1; 4; {10 +
    // 4 + 7 + 47 + 4} -> 76; {40} -> 116; {40} -> 156.
    let set = vec![cell("note", attr(Value::str("xyz")))];
    let branch = assert_record_write(
        &engine,
        ns,
        &WriteVector::new(
            "patch a note -> attribute",
            patch_record(ns, "a", set.clone(), Vec::new()),
            vec![
                lookup(),
                point_read(),
                patch_group(1, 7, 0, 47),
                insert(true),
                insert(true),
            ],
            1 + 3 + (10 + 4 + 7 + 47 + 4) + 40 + 40,
            Expect::Applied(Effect::RecordPatched { id: rid(1) }),
        ),
    );
    let mut expected = model;
    expected.patch("a", &set, &[]);
    assert_eq!(expected.term_count(), 11);
    assert_eq!(observe(&branch, ns), modelled(&expected));
}

#[test]
fn patch_setting_an_identical_attribute_touches_no_term() {
    let (engine, ns, model) = fixture();
    // set kind := attribute "book" (unchanged): old == new, so only the
    // patch group is charged, and the row is still rewritten: 1 + 3 + (10 +
    // 4 + 8 + 47 + 4) = 77.
    let branch = assert_record_write(
        &engine,
        ns,
        &WriteVector::new(
            "patch a kind := book (same)",
            patch_record(
                ns,
                "a",
                vec![cell("kind", attr(Value::str("book")))],
                Vec::new(),
            ),
            vec![lookup(), point_read(), patch_group(1, 8, 0, 47)],
            1 + 3 + (10 + 4 + 8 + 47 + 4),
            Expect::Applied(Effect::RecordPatched { id: rid(1) }),
        ),
    );
    assert_eq!(observe(&branch, ns), modelled(&model));

    // The same for a value just written: price := I64(11) twice. The second
    // patch is 1 + 3 + 78 = 82 and nothing else.
    let set = vec![cell("price", attr(Value::I64(11)))];
    let branch = assert_record_write(
        &engine,
        ns,
        &WriteVector::new(
            "patch a price := 11 (same)",
            patch_record(ns, "a", set.clone(), Vec::new()),
            vec![lookup(), point_read(), patch_group(1, 13, 0, 47)],
            82,
            Expect::Applied(Effect::RecordPatched { id: rid(1) }),
        )
        .setup(&[patch_record(ns, "a", set.clone(), Vec::new())]),
    );
    let mut expected = model;
    expected.patch("a", &set, &[]);
    assert_eq!(observe(&branch, ns), modelled(&expected));
}

#[test]
fn patch_that_would_overfill_the_record_is_record_full_after_the_lookup() {
    // max_cells 4: the fixture records are exactly full.
    let limits = Limits {
        max_cells: 4,
        ..Limits::DEFAULT
    };
    let (engine, ns, model) = fixture_with(limits);
    assert_eq!(engine.limits(), limits);
    let color = cell("color", attr(Value::Bool(true)));

    // |existing| 4 - |remove ∩ existing| 0 + |set \ existing| 1 = 5 > 4:
    // RecordFull at 1 + 3 = 4, recorded, before the patch group.
    let branch = assert_record_write(
        &engine,
        ns,
        &WriteVector::new(
            "patch a + color (full)",
            patch_record(ns, "a", vec![color.clone()], Vec::new()),
            vec![lookup(), point_read()],
            1 + 3,
            Expect::Rejected(Rejection::RecordFull),
        ),
    );
    assert_eq!(observe(&branch, ns), modelled(&model));
    assert_eq!(
        branch.receipts(),
        &[write_receipt(4, Outcome::Rejected(Rejection::RecordFull))]
    );

    // Removing an absent name does not make room: still 5 > 4.
    assert_record_write(
        &engine,
        ns,
        &WriteVector::new(
            "patch a + color - missing (full)",
            patch_record(ns, "a", vec![color.clone()], names(&["missing"])),
            vec![lookup(), point_read()],
            1 + 3,
            Expect::Rejected(Rejection::RecordFull),
        ),
    );

    // Removing a held name does: 4 - 1 + 1 = 4 applies. s = 1, b = 5 + 1 =
    // 6, r = 1, n = 1 + 18 + 6 + 8 + 13 = 46: 1; 4; {10 + 4 + 6 + 4 + 46 +
    // 4} -> 78; color: both terms new -> 118, 158; note: field -> nothing.
    let branch = assert_record_write(
        &engine,
        ns,
        &WriteVector::new(
            "patch a + color - note",
            patch_record(ns, "a", vec![color.clone()], names(&["note"])),
            vec![
                lookup(),
                point_read(),
                patch_group(1, 6, 1, 46),
                insert(true),
                insert(true),
            ],
            1 + 3 + (10 + 4 + 6 + 4 + 46 + 4) + 40 + 40,
            Expect::Applied(Effect::RecordPatched { id: rid(1) }),
        ),
    );
    let mut expected = model.clone();
    expected.patch("a", &[color], &names(&["note"]));
    assert_eq!(observe(&branch, ns), modelled(&expected));

    // Removing one held name does not make room for two new ones:
    // 4 - 1 (note) + 2 (x, y) = 5: still full.
    let x = cell("x", attr(Value::Bool(true)));
    let y = cell("y", attr(Value::Bool(false)));
    assert_record_write(
        &engine,
        ns,
        &WriteVector::new(
            "patch a + x + y - note (full)",
            patch_record(ns, "a", vec![x.clone(), y], names(&["note"])),
            vec![lookup(), point_read()],
            4,
            Expect::Rejected(Rejection::RecordFull),
        ),
    );
    // 4 - 1 (a removed field) + 1 (x) = 4: admitted and applied.
    // 1 + 3 + {RecordPatch 10 + CellWrite 4 + CellByte (1 + 1) + CellRemove 4
    // + RowByte 42 (key 1 + $expiresAt 18 + kind 8 + price 13 + x 2) +
    // RootUpdate 2*2} = 70; note: nothing; x: two new terms {6 + 30 + 4}
    // each -> 150.
    let make_room = patch_record(ns, "a", vec![x.clone()], names(&["note"]));
    assert_record_write(
        &engine,
        ns,
        &WriteVector::new(
            "patch a + x - note",
            make_room.clone(),
            vec![
                lookup(),
                point_read(),
                patch_group(1, 2, 1, 42),
                insert(true),
                insert(true),
            ],
            150,
            Expect::Applied(Effect::RecordPatched { id: rid(1) }),
        ),
    );
    // Replacing an existing name keeps the count: 4 -> 4. kind attribute ->
    // field (the row stays 42 bytes): 1 + 3 + {10 + 4 + 8 + 42 + 4} = 72,
    // then {IndexRemove 6 + RootUpdate 4} twice -> 92.
    let replace = vec![cell("kind", field(Value::str("book")))];
    let branch = assert_record_write(
        &engine,
        ns,
        &WriteVector::new(
            "patch a kind -> field (full record)",
            patch_record(ns, "a", replace.clone(), Vec::new()),
            vec![
                lookup(),
                point_read(),
                patch_group(1, 8, 0, 42),
                index_remove(),
                index_remove(),
            ],
            92,
            Expect::Applied(Effect::RecordPatched { id: rid(1) }),
        )
        .setup(std::slice::from_ref(&make_room)),
    );
    let mut expected = model.clone();
    expected.patch("a", &[x], &names(&["note"]));
    expected.patch("a", &replace, &[]);
    assert_eq!(observe(&branch, ns), modelled(&expected));

    // Naming more than max_cells cells is an admission fault: spent 0, not
    // recorded, no charge at all even under an unlimited budget.
    let mut branch = begin(&engine, None);
    let too_many = patch_record(
        ns,
        "a",
        vec![
            cell("c1", attr(Value::Bool(true))),
            cell("c2", attr(Value::Bool(true))),
            cell("c3", attr(Value::Bool(true))),
            cell("c4", attr(Value::Bool(true))),
        ],
        names(&["note"]),
    );
    let rejected = branch
        .execute(ALICE, too_many, Budget::unlimited().traced())
        .expect_err("5 names exceed max_cells 4");
    let fault = Rejection::InvalidArgument(ArgumentFault::TooManyCells);
    assert_eq!(rejected.reason, fault);
    assert_eq!(
        rejected.receipt,
        traced(write_receipt(0, Outcome::Rejected(fault)), ledger(&[]))
    );
    assert!(
        branch.receipts().is_empty(),
        "admission faults are not recorded"
    );
    assert_eq!(branch.spent(), Cost::ZERO);
    assert_eq!(observe(&branch, ns), modelled(&model));
}

#[test]
fn delete_charges_the_removal_group_then_one_unit_per_held_term() {
    let (engine, ns, model) = fixture();
    // delete a (4 cells, 3 attributes): 1; 4; {RecordDelete 20 + CellRemove
    // 16 + RootUpdate 3*2} -> 46; per attribute in name order value then
    // existence: {IndexRemove 6 + RootUpdate 2*2} x 6 -> 56, 66, 76, 86, 96,
    // 106. No byte charge.
    let branch = assert_record_write(
        &engine,
        ns,
        &WriteVector::new(
            "delete a",
            delete_record(ns, "a"),
            vec![
                lookup(),
                point_read(),
                delete_group(4),
                index_remove(),
                index_remove(),
                index_remove(),
                index_remove(),
                index_remove(),
                index_remove(),
            ],
            1 + 3 + (20 + 16 + 6) + 10 + 10 + 10 + 10 + 10 + 10,
            Expect::Applied(Effect::RecordDeleted { id: rid(1) }),
        ),
    );
    let mut expected = model;
    expected.delete("a");
    assert_eq!(expected.term_count(), 8, "price 10 was held by a alone");
    assert_eq!(observe(&branch, ns), modelled(&expected));
    // The traced ledger, spelled out.
    let mut branch = begin(&engine, None);
    let executed = branch
        .execute(ALICE, delete_record(ns, "a"), Budget::limit(106).traced())
        .expect("106 is exact");
    assert_eq!(
        executed.receipt,
        traced(
            write_receipt(106, Outcome::Applied),
            ledger(&[
                (OperationClass::DirectoryLookup, 1, 1, 1),
                (OperationClass::PointRead, 1, 3, 3),
                (OperationClass::RecordDelete, 1, 20, 20),
                (OperationClass::CellRemove, 4, 4, 16),
                (OperationClass::RootUpdate, 3 + 6 * 2, 2, 30),
                (OperationClass::IndexRemove, 6, 6, 36),
            ])
        )
    );

    // After the worked patch a has 3 cells and 3 attributes: 1 + 3 + (20 +
    // 12 + 6) + 6 * 10 = 102 (the first expiry delete).
    let worked_patch = patch_record(
        ns,
        "a",
        vec![cell("price", attr(Value::U64(10)))],
        names(&["note", "missing"]),
    );
    let branch = assert_record_write(
        &engine,
        ns,
        &WriteVector::new(
            "delete a (3 cells)",
            delete_record(ns, "a"),
            vec![
                lookup(),
                point_read(),
                delete_group(3),
                index_remove(),
                index_remove(),
                index_remove(),
                index_remove(),
                index_remove(),
                index_remove(),
            ],
            1 + 3 + (20 + 12 + 6) + 10 + 10 + 10 + 10 + 10 + 10,
            Expect::Applied(Effect::RecordDeleted { id: rid(1) }),
        )
        .setup(std::slice::from_ref(&worked_patch)),
    );
    assert_eq!(observe(&branch, ns), modelled(&expected));

    // Deleting every record empties the index: terms return to zero and a
    // recreated key gets a fresh id.
    let mut branch = begin(&engine, None);
    for key_text in ["a", "b", "c"] {
        branch
            .execute(ALICE, delete_record(ns, key_text), Budget::limit(106))
            .expect("106 is exact for every fixture record");
    }
    assert_eq!(branch.typed_record_count(ns), Some(0));
    assert_eq!(branch.term_count(ns), Some(0));
    // create a again: six new terms, as on the empty namespace: 379, id 4.
    let executed = branch
        .execute(
            ALICE,
            create_record(ns, "a", fixture_cells("book", 10)),
            Budget::unlimited(),
        )
        .expect("applies");
    assert_eq!(executed.effect, Effect::RecordCreated { id: rid(4) });
    assert_eq!(executed.receipt, write_receipt(379, Outcome::Applied));
}

#[test]
fn typed_writes_rejected_after_the_binding_lookup_spend_four() {
    let (engine, ns, model) = fixture();
    // DirectoryLookup 1; PointRead 3 -> 4; then the binding decides. The
    // rejection is recorded and the state unchanged; budgets 0..=3 are
    // OutOfBudget at 0 or 1.
    let vectors = [
        // create a again: the key is bound.
        (
            "create a (bound)",
            create_record(ns, "a", fixture_cells("pen", 99)),
            Rejection::AlreadyExists,
        ),
        // patch z and delete z: unbound.
        (
            "patch z (unbound)",
            patch_record(
                ns,
                "z",
                vec![cell("kind", attr(Value::str("ink")))],
                Vec::new(),
            ),
            Rejection::NotFound,
        ),
        (
            "patch z remove-only (unbound)",
            patch_record(ns, "z", Vec::new(), names(&["kind"])),
            Rejection::NotFound,
        ),
        (
            "delete z (unbound)",
            delete_record(ns, "z"),
            Rejection::NotFound,
        ),
    ];
    for (name, op, reason) in vectors {
        let branch = assert_record_write(
            &engine,
            ns,
            &WriteVector::new(
                name,
                op,
                vec![lookup(), point_read()],
                1 + 3,
                Expect::Rejected(reason),
            ),
        );
        assert_eq!(observe(&branch, ns), modelled(&model), "{name}");
        assert_eq!(
            branch.receipts(),
            &[write_receipt(4, Outcome::Rejected(reason))],
            "{name}"
        );
    }
    // A deleted key is unbound too: delete a, then delete a again.
    let branch = assert_record_write(
        &engine,
        ns,
        &WriteVector::new(
            "delete a twice",
            delete_record(ns, "a"),
            vec![lookup(), point_read()],
            1 + 3,
            Expect::Rejected(Rejection::NotFound),
        )
        .setup(&[delete_record(ns, "a")]),
    );
    assert_eq!(
        branch.receipts(),
        &[
            write_receipt(106, Outcome::Applied),
            write_receipt(4, Outcome::Rejected(Rejection::NotFound)),
        ]
    );
}

#[test]
fn typed_writes_reject_wrong_owner_and_unknown_namespace_after_the_lookup() {
    let (engine, ns, model) = fixture();
    let ops = |target| {
        [
            (
                "create",
                create_record(target, "d", fixture_cells("book", 10)),
            ),
            (
                "patch",
                patch_record(
                    target,
                    "a",
                    vec![cell("price", attr(Value::I64(11)))],
                    Vec::new(),
                ),
            ),
            ("delete", delete_record(target, "a")),
        ]
    };
    // DirectoryLookup 1, then the rejection: BOB is not the owner, and
    // ordinal 9 does not exist.
    let cases = [
        (ns, BOB, "by BOB", Rejection::Unauthorized),
        (
            vocab::ns(9),
            ALICE,
            "on ordinal 9",
            Rejection::UnknownNamespace,
        ),
    ];
    for (target, actor, suffix, reason) in cases {
        for (label, op) in ops(target) {
            let branch = assert_record_write(
                &engine,
                ns,
                &WriteVector::new(
                    &format!("{label} {suffix}"),
                    op,
                    vec![lookup()],
                    1,
                    Expect::Rejected(reason),
                )
                .actor(actor),
            );
            assert_eq!(observe(&branch, ns), modelled(&model));
        }
    }
}

#[test]
fn get_record_vectors_for_every_projection() {
    let (engine, ns, model) = fixture();
    let snapshot = engine.head_snapshot();
    let branch = begin(&engine, None);
    // (label, key, projection, groups, total): DirectoryLookup 1; PointRead
    // 3 -> 4; unbound -> None; bound -> {ResultRow 3 + ReadByte p}.
    let vectors: Vec<(&str, &str, Projection, Vec<Group>, u64)> = vec![
        (
            "missing",
            "z",
            Projection::ALL,
            vec![lookup(), point_read()],
            1 + 3,
        ),
        (
            // p = 0.
            "id only",
            "a",
            Projection::ID_ONLY,
            vec![lookup(), point_read(), emit(0)],
            1 + 3 + 3,
        ),
        (
            // p = key "a" 1.
            "key only",
            "a",
            Projection::KEY_ONLY,
            vec![lookup(), point_read(), emit(1)],
            1 + 3 + (3 + 1),
        ),
        (
            // p = 1 + 18 + 8 + 7 + 13 = 47 (total 54).
            "all",
            "a",
            Projection::ALL,
            vec![lookup(), point_read(), emit(47)],
            1 + 3 + (3 + 47),
        ),
        (
            // c: kind pen is 3 bytes: p = 1 + 18 + 7 + 7 + 13 = 46.
            "all c",
            "c",
            Projection::ALL,
            vec![lookup(), point_read(), emit(46)],
            1 + 3 + (3 + 46),
        ),
        (
            // Named [price, missing] without the key: only price (13) is
            // projected; the missing name costs nothing.
            "named price + absent",
            "a",
            named(false, &["price", "missing"]),
            vec![lookup(), point_read(), emit(13)],
            1 + 3 + (3 + 13),
        ),
        (
            // Named [kind, note] with the key: 1 + 8 + 7 = 16 (the field
            // is projected like any cell).
            "named key + kind + note",
            "a",
            named(true, &["kind", "note"]),
            vec![lookup(), point_read(), emit(16)],
            1 + 3 + (3 + 16),
        ),
        (
            // Absent names with the key project the key only (p = 1).
            "named key + absent",
            "a",
            named(true, &["missing", "zzz"]),
            vec![lookup(), point_read(), emit(1)],
            8,
        ),
        (
            // Named [] : p = 0 like ID_ONLY.
            "named empty",
            "b",
            named(false, &[]),
            vec![lookup(), point_read(), emit(0)],
            1 + 3 + 3,
        ),
    ];
    for (label, key_text, projection, groups, total) in &vectors {
        let expected = model.row(key_text, projection);
        assert_read_vector(
            &format!("snapshot get_record {label}"),
            groups,
            *total,
            &expected,
            |budget| snapshot.get_record(ns, &key(key_text), projection, budget),
        );
        assert_read_vector(
            &format!("branch get_record {label}"),
            groups,
            *total,
            &expected,
            |budget| branch.get_record(ns, &key(key_text), projection, budget),
        );
    }
    // The projected rows are the literal shapes.
    let row = model
        .row("a", &named(true, &["kind", "note"]))
        .expect("a is live");
    assert_eq!(
        row,
        Row {
            id: rid(1),
            key: Some(key("a")),
            cells: vec![
                cell("kind", attr(Value::str("book"))),
                cell("note", field(Value::bytes(b"xyz"))),
            ],
        }
    );
    assert_eq!(
        model.row("a", &Projection::ID_ONLY),
        Some(Row {
            id: rid(1),
            key: None,
            cells: Vec::new(),
        })
    );
    assert!(branch.receipts().is_empty(), "reads are never recorded");
}

/// Runs a cursor-less query vector on the head snapshot and on a fresh
/// branch; both must charge and return the same.
fn assert_query_on_both(engine: &Engine, ns: NamespaceId, model: &Model, vector: &QueryVector<'_>) {
    assert!(
        vector.after.is_none(),
        "continuations are checked explicitly"
    );
    let snapshot = engine.head_snapshot();
    let branch = begin(engine, None);
    assert_query_vector(model, vector, |budget| {
        snapshot.query(
            ns,
            &vector.query,
            &vector.projection,
            vector.limit,
            None,
            budget,
        )
    });
    assert_query_vector(model, vector, |budget| {
        branch.query(
            ns,
            &vector.query,
            &vector.projection,
            vector.limit,
            None,
            budget,
        )
    });
}

#[test]
fn eq_query_pages_charge_the_head_beyond_a_full_page() {
    let (engine, ns, model) = fixture();
    let query = Query::term(eq("kind", Value::str("book")));
    // The worked vector. Page 1, ALL, limit 1: DirectoryLookup 1;
    // QueryGroup 5 -> 6; TermSeek 4 -> 10; PostingEntry (id 1, the head)
    // -> 11; {ResultRow 3 + ReadByte 47} -> 61; advance: PostingEntry (id 2,
    // the head beyond the page) -> 62; the page is full: next after 1.
    let page1 = QueryVector::new(
        "Eq(kind, book) page 1",
        query.clone(),
        Projection::ALL,
        vec![
            lookup(),
            query_group(),
            term_seek(),
            posting(),
            emit(47),
            posting(),
        ],
        1 + 5 + 4 + 1 + (3 + 47) + 1,
        &[1],
    )
    .limit(1)
    .next(1);
    assert_query_on_both(&engine, ns, &model, &page1);

    // Page 2 with the cursor: the posting is positioned after 1: PostingEntry
    // (id 2) -> 11; emit b (47 bytes) -> 61; the posting is exhausted (no
    // charge): next None.
    let page2 = QueryVector::new(
        "Eq(kind, book) page 2",
        query.clone(),
        Projection::ALL,
        vec![lookup(), query_group(), term_seek(), posting(), emit(47)],
        1 + 5 + 4 + 1 + (3 + 47),
        &[2],
    )
    .limit(1)
    .after(rid(1));
    let snapshot = engine.head_snapshot();
    let first = snapshot
        .query(ns, &query, &Projection::ALL, 1, None, Budget::unlimited())
        .expect("applies");
    let cursor = first.value.next.expect("a further match exists");
    assert_eq!(cursor.after(), rid(1));
    assert_eq!(cursor.snapshot(), Some(snapshot.id()));
    assert_query_vector(&model, &page2, |budget| {
        snapshot.query(ns, &query, &Projection::ALL, 1, Some(&cursor), budget)
    });
    // The same continuation on a branch, with the branch's own cursor.
    let branch = begin(&engine, None);
    let first = branch
        .query(ns, &query, &Projection::ALL, 1, None, Budget::unlimited())
        .expect("applies");
    let cursor = first.value.next.expect("a further match exists");
    assert_eq!(cursor.snapshot(), None, "a branch cursor names no snapshot");
    assert_query_vector(&model, &page2, |budget| {
        branch.query(ns, &query, &Projection::ALL, 1, Some(&cursor), budget)
    });

    // One page of limit 10 holds both: PostingEntry 1 -> 11; emit -> 61;
    // PostingEntry 2 -> 62; emit -> 112; exhausted: next None.
    assert_query_on_both(
        &engine,
        ns,
        &model,
        &QueryVector::new(
            "Eq(kind, book) limit 10",
            query.clone(),
            Projection::ALL,
            vec![
                lookup(),
                query_group(),
                term_seek(),
                posting(),
                emit(47),
                posting(),
                emit(47),
            ],
            1 + 5 + 4 + 1 + (3 + 47) + 1 + (3 + 47),
            &[1, 2],
        ),
    );
    // A page exactly full with no further match: limit 2 costs the same and
    // claims next None.
    assert_query_on_both(
        &engine,
        ns,
        &model,
        &QueryVector::new(
            "Eq(kind, book) limit 2 (exactly full)",
            query,
            Projection::ALL,
            vec![
                lookup(),
                query_group(),
                term_seek(),
                posting(),
                emit(47),
                posting(),
                emit(47),
            ],
            1 + 5 + 4 + 1 + (3 + 47) + 1 + (3 + 47),
            &[1, 2],
        )
        .limit(2),
    );
    // A term nobody holds costs the seek only: 1 + 5 + 4 = 10, no rows.
    assert_seek_only(
        &engine,
        ns,
        &model,
        "Eq(kind, ink)",
        Query::term(eq("kind", Value::str("ink"))),
        Projection::ALL,
    );
    // So does an existing value of another type: Eq(price, U64(10)).
    assert_seek_only(
        &engine,
        ns,
        &model,
        "Eq(price, U64 10)",
        Query::term(eq("price", Value::U64(10))),
        Projection::ID_ONLY,
    );
    // The traced ledger of page 1, spelled out.
    let traced_page = engine
        .head_snapshot()
        .query(
            ns,
            &Query::term(eq("kind", Value::str("book"))),
            &Projection::ALL,
            1,
            None,
            Budget::limit(62).traced(),
        )
        .expect("62 is exact");
    assert_eq!(
        traced_page.receipt,
        traced(
            read_receipt(62, Outcome::Applied),
            ledger(&[
                (OperationClass::DirectoryLookup, 1, 1, 1),
                (OperationClass::QueryGroup, 1, 5, 5),
                (OperationClass::TermSeek, 1, 4, 4),
                (OperationClass::PostingEntry, 2, 1, 2),
                (OperationClass::ResultRow, 1, 3, 3),
                (OperationClass::ReadByte, 47, 1, 47),
            ])
        )
    );
}

#[test]
fn exists_and_not_eq_queries_seek_the_existence_posting() {
    let (engine, ns, model) = fixture();
    // Exists(kind, Str), ID_ONLY, limit 10: seek the existence posting
    // {1, 2, 3}: 1 + 5 + 4 = 10; then PostingEntry 1 + ResultRow 3 per row
    // -> 14, 18, 22; exhausted.
    assert_query_on_both(
        &engine,
        ns,
        &model,
        &QueryVector::new(
            "Exists(kind, Str)",
            Query::term(exists("kind", TypeTag::Str)),
            Projection::ID_ONLY,
            vec![
                lookup(),
                query_group(),
                term_seek(),
                posting(),
                emit(0),
                posting(),
                emit(0),
                posting(),
                emit(0),
            ],
            1 + 5 + 4 + (1 + 3) + (1 + 3) + (1 + 3),
            &[1, 2, 3],
        ),
    );
    // Exists(kind, I64): no such existence term: the seek only.
    assert_seek_only(
        &engine,
        ns,
        &model,
        "Exists(kind, I64)",
        Query::term(exists("kind", TypeTag::I64)),
        Projection::ID_ONLY,
    );
    // NotEq(kind, book), KEY_ONLY: the existence posting {1, 2, 3} is only
    // a domain, so the literal itself probes every candidate: PostingEntry
    // 1 + RecordProbe 2 for 1 (fails), 2 (fails), 3 (passes) -> 19; emit
    // {3 + 1} -> 23; exhausted.
    assert_query_on_both(
        &engine,
        ns,
        &model,
        &QueryVector::new(
            "NotEq(kind, book)",
            Query::term(not_eq("kind", Value::str("book"))),
            Projection::KEY_ONLY,
            vec![
                lookup(),
                query_group(),
                term_seek(),
                posting(),
                probe(),
                posting(),
                probe(),
                posting(),
                probe(),
                emit(1),
            ],
            1 + 5 + 4 + (1 + 2) + (1 + 2) + (1 + 2) + (3 + 1),
            &[3],
        ),
    );
    // NotEq(price, U64(5)): no record holds a U64 price, so nothing is
    // probed: the seek only (a record lacking the type does not match).
    assert_seek_only(
        &engine,
        ns,
        &model,
        "NotEq(price, U64 5)",
        Query::term(not_eq("price", Value::U64(5))),
        Projection::KEY_ONLY,
    );
}

#[test]
fn range_queries_visit_only_the_qualifying_terms() {
    let (engine, ns, model) = fixture();
    // The (price, I64) run in key order: existence, 5 {3}, 10 {1}, 20 {2}.
    // The worked vector: Excluded(5)..Unbounded, ID_ONLY, limit 10: 1 + 5 +
    // 4 = 10; the seek lands on 5, which is stepped over unpaid; TermVisit
    // 10 -> 13, 20 -> 16; the run ends at the next name. Heads by id:
    // PostingEntry (1) -> 17; emit -> 20; PostingEntry (2) -> 21; emit -> 24.
    let run_then_two_rows = |bytes: u64| {
        vec![
            lookup(),
            query_group(),
            term_seek(),
            term_visit(),
            term_visit(),
            posting(),
            emit(bytes),
            posting(),
            emit(bytes),
        ]
    };
    assert_query_on_both(
        &engine,
        ns,
        &model,
        &QueryVector::new(
            "Range(price, >5, ..)",
            Query::term(range(
                "price",
                Bound::Excluded(Value::I64(5)),
                Bound::Unbounded,
            )),
            Projection::ID_ONLY,
            run_then_two_rows(0),
            1 + 5 + 4 + 3 + 3 + (1 + 3) + (1 + 3),
            &[1, 2],
        ),
    );
    // Included(5)..Excluded(20): 5 and 10 are visited; 20 exists, is out of
    // range and terminates the run unpaid. Rows 1 (price 10) and 3 (price
    // 5) come from two postings merged by id.
    assert_query_on_both(
        &engine,
        ns,
        &model,
        &QueryVector::new(
            "Range(price, 5..20)",
            Query::term(range(
                "price",
                Bound::Included(Value::I64(5)),
                Bound::Excluded(Value::I64(20)),
            )),
            Projection::ID_ONLY,
            run_then_two_rows(0),
            1 + 5 + 4 + 3 + 3 + (1 + 3) + (1 + 3),
            &[1, 3],
        ),
    );
    // Unbounded..Included(10): the run starts at the first value term (the
    // existence term is never visited): 5, 10 visited; 20 terminates.
    assert_query_on_both(
        &engine,
        ns,
        &model,
        &QueryVector::new(
            "Range(price, ..=10)",
            Query::term(range(
                "price",
                Bound::Unbounded,
                Bound::Included(Value::I64(10)),
            )),
            Projection::ID_ONLY,
            run_then_two_rows(0),
            1 + 5 + 4 + 3 + 3 + (1 + 3) + (1 + 3),
            &[1, 3],
        ),
    );
    // Excluded(10)..Included(20): the seek lands on 10 (stepped over); 20
    // is visited; then the next name ends the run: one term, one row.
    assert_query_on_both(
        &engine,
        ns,
        &model,
        &QueryVector::new(
            "Range(price, 10<..=20)",
            Query::term(range(
                "price",
                Bound::Excluded(Value::I64(10)),
                Bound::Included(Value::I64(20)),
            )),
            Projection::KEY_ONLY,
            vec![
                lookup(),
                query_group(),
                term_seek(),
                term_visit(),
                posting(),
                emit(1),
            ],
            1 + 5 + 4 + 3 + 1 + (3 + 1),
            &[2],
        ),
    );
    // lower > upper: the seek lands on 20, which is above the upper bound
    // 10, so the run is empty: the seek only.
    assert_seek_only(
        &engine,
        ns,
        &model,
        "Range(price, 20..=10)",
        Query::term(range(
            "price",
            Bound::Included(Value::I64(20)),
            Bound::Included(Value::I64(10)),
        )),
        Projection::ID_ONLY,
    );
    // A run past every term of the type: $expiresAt >= 6 seeks past the
    // only value (5) and stops at the next name unpaid.
    assert_seek_only(
        &engine,
        ns,
        &model,
        "Range($expiresAt, >=6)",
        Query::term(range(
            "$expiresAt",
            Bound::Included(Value::U64(6)),
            Bound::Unbounded,
        )),
        Projection::ID_ONLY,
    );
    // A range on a type with no terms of that name: the seek only.
    assert_seek_only(
        &engine,
        ns,
        &model,
        "Range(price, U64)",
        Query::term(range(
            "price",
            Bound::Included(Value::U64(0)),
            Bound::Unbounded,
        )),
        Projection::ID_ONLY,
    );
    // A one-term run, Range($expiresAt, ..=U64(5)), KEY_ONLY, limit 200:
    // one term visited (5 {1, 2, 3}), then PostingEntry 1 + {3 + 1} per row.
    assert_query_on_both(
        &engine,
        ns,
        &model,
        &QueryVector::new(
            "Range($expiresAt, ..=5)",
            Query::term(range(
                "$expiresAt",
                Bound::Unbounded,
                Bound::Included(Value::U64(5)),
            )),
            Projection::KEY_ONLY,
            vec![
                lookup(),
                query_group(),
                term_seek(),
                term_visit(),
                posting(),
                emit(1),
                posting(),
                emit(1),
                posting(),
                emit(1),
            ],
            1 + 5 + 4 + 3 + (1 + 4) + (1 + 4) + (1 + 4),
            &[1, 2, 3],
        )
        .limit(200),
    );
}

#[test]
fn starts_with_queries_visit_the_prefix_run() {
    let (engine, ns, model) = fixture();
    // The (kind, Str) run: existence, "book" {1, 2}, "pen" {3}.
    // StartsWith(kind, "b"), ALL: TermVisit book -> 13; "pen" terminates
    // unpaid; PostingEntry 1 -> 14; emit 47 -> 64; PostingEntry -> 65; emit
    // -> 115.
    assert_query_on_both(
        &engine,
        ns,
        &model,
        &QueryVector::new(
            "StartsWith(kind, b)",
            Query::term(starts_with("kind", "b")),
            Projection::ALL,
            vec![
                lookup(),
                query_group(),
                term_seek(),
                term_visit(),
                posting(),
                emit(47),
                posting(),
                emit(47),
            ],
            1 + 5 + 4 + 3 + 1 + (3 + 47) + 1 + (3 + 47),
            &[1, 2],
        ),
    );
    // The empty prefix visits every Str value term of the name (never the
    // existence term): book, pen -> 16; then the merged postings: 1, 2 from
    // book and 3 from pen, each PostingEntry 1 + ResultRow 3.
    assert_query_on_both(
        &engine,
        ns,
        &model,
        &QueryVector::new(
            "StartsWith(kind, \"\")",
            Query::term(starts_with("kind", "")),
            Projection::ID_ONLY,
            vec![
                lookup(),
                query_group(),
                term_seek(),
                term_visit(),
                term_visit(),
                posting(),
                emit(0),
                posting(),
                emit(0),
                posting(),
                emit(0),
            ],
            1 + 5 + 4 + 3 + 3 + (1 + 3) + (1 + 3) + (1 + 3),
            &[1, 2, 3],
        ),
    );
    // A prefix with no term: "c" seeks between book and pen; pen does not
    // start with c and terminates the run unpaid.
    assert_seek_only(
        &engine,
        ns,
        &model,
        "StartsWith(kind, c)",
        Query::term(starts_with("kind", "c")),
        Projection::ID_ONLY,
    );
    // A prefix past every value: "z" seeks to the next name.
    assert_seek_only(
        &engine,
        ns,
        &model,
        "StartsWith(kind, z)",
        Query::term(starts_with("kind", "z")),
        Projection::ID_ONLY,
    );
    // A prefix on a name with no Str terms (price): the seek only.
    assert_seek_only(
        &engine,
        ns,
        &model,
        "StartsWith(price, 1)",
        Query::term(starts_with("price", "1")),
        Projection::ID_ONLY,
    );
}

#[test]
fn negated_first_literal_and_match_all_walk_the_row_map() {
    let (engine, ns, model) = fixture();
    // The worked vector: NOT StartsWith(kind, "bo"), KEY_ONLY: no seek; the
    // row map is the source and the literal probes each row: RowVisit 1 +
    // RecordProbe 2 for 1 (fails), 2 (fails), 3 (passes) -> 15; emit {3 +
    // 1} -> 19; exhausted.
    assert_query_on_both(
        &engine,
        ns,
        &model,
        &QueryVector::new(
            "NOT StartsWith(kind, bo)",
            Query::all(vec![Literal::not(starts_with("kind", "bo"))]),
            Projection::KEY_ONLY,
            vec![
                lookup(),
                query_group(),
                row_visit(),
                probe(),
                row_visit(),
                probe(),
                row_visit(),
                probe(),
                emit(1),
            ],
            1 + 5 + (1 + 2) + (1 + 2) + (1 + 2) + (3 + 1),
            &[3],
        ),
    );
    // NOT Exists(color, Bool): every row passes: RowVisit + RecordProbe +
    // emit per row.
    assert_query_on_both(
        &engine,
        ns,
        &model,
        &QueryVector::new(
            "NOT Exists(color, Bool)",
            Query::all(vec![Literal::not(exists("color", TypeTag::Bool))]),
            Projection::ID_ONLY,
            vec![
                lookup(),
                query_group(),
                row_visit(),
                probe(),
                emit(0),
                row_visit(),
                probe(),
                emit(0),
                row_visit(),
                probe(),
                emit(0),
            ],
            1 + 5 + (1 + 2 + 3) + (1 + 2 + 3) + (1 + 2 + 3),
            &[1, 2, 3],
        ),
    );
    // match-all, ID_ONLY, limit 10: RowVisit 1 + ResultRow 3 per row: 6, 10,
    // 14, 18; exhausted.
    let match_all_rows = vec![
        lookup(),
        query_group(),
        row_visit(),
        emit(0),
        row_visit(),
        emit(0),
        row_visit(),
        emit(0),
    ];
    assert_query_on_both(
        &engine,
        ns,
        &model,
        &QueryVector::new(
            "match-all limit 10",
            Query::match_all(),
            Projection::ID_ONLY,
            match_all_rows.clone(),
            1 + 5 + (1 + 3) + (1 + 3) + (1 + 3),
            &[1, 2, 3],
        ),
    );
    // A page exactly full with nothing beyond: limit 3 costs the same; the
    // exhausted row map costs nothing and the claim is next None.
    assert_query_on_both(
        &engine,
        ns,
        &model,
        &QueryVector::new(
            "match-all limit 3 (exactly full)",
            Query::match_all(),
            Projection::ID_ONLY,
            match_all_rows,
            1 + 5 + (1 + 3) + (1 + 3) + (1 + 3),
            &[1, 2, 3],
        )
        .limit(3),
    );
    // A page exactly full with a further match: limit 2 visits row 3 (the
    // head beyond the page) and stops: 6 + 4 + 4 + 1 = 15, next after 2.
    assert_query_on_both(
        &engine,
        ns,
        &model,
        &QueryVector::new(
            "match-all limit 2 (exactly full)",
            Query::match_all(),
            Projection::ID_ONLY,
            vec![
                lookup(),
                query_group(),
                row_visit(),
                emit(0),
                row_visit(),
                emit(0),
                row_visit(),
            ],
            1 + 5 + (1 + 3) + (1 + 3) + 1,
            &[1, 2],
        )
        .limit(2)
        .next(2),
    );
    // The continuation after 2 with limit 2: RowVisit 3 -> 7; emit -> 10;
    // exhausted: next None. The limit is not bound by the cursor: limit 1
    // charges the same here because nothing follows.
    let snapshot = engine.head_snapshot();
    let first = snapshot
        .query(
            ns,
            &Query::match_all(),
            &Projection::ID_ONLY,
            2,
            None,
            Budget::unlimited(),
        )
        .expect("applies");
    let cursor = first.value.next.expect("row 3 follows");
    for limit in [2, 1] {
        assert_query_vector(
            &model,
            &QueryVector::new(
                "match-all continuation",
                Query::match_all(),
                Projection::ID_ONLY,
                vec![lookup(), query_group(), row_visit(), emit(0)],
                1 + 5 + 1 + 3,
                &[3],
            )
            .limit(limit)
            .after(rid(2)),
            |budget| {
                snapshot.query(
                    ns,
                    &Query::match_all(),
                    &Projection::ID_ONLY,
                    limit,
                    Some(&cursor),
                    budget,
                )
            },
        );
    }
}

#[test]
fn and_groups_probe_each_candidate_until_a_literal_fails() {
    let (engine, ns, model) = fixture();
    // [Eq(kind, book), Eq(price, 20)], KEY_ONLY: the first literal is the
    // source (posting {1, 2}); the second probes: PostingEntry 1 + probe 2
    // for 1 (price 10 fails) -> 13; for 2 (passes) -> 16; emit {3 + 1} ->
    // 20; exhausted.
    assert_query_on_both(
        &engine,
        ns,
        &model,
        &QueryVector::new(
            "book AND price 20",
            Query::all(vec![
                Literal::of(eq("kind", Value::str("book"))),
                Literal::of(eq("price", Value::I64(20))),
            ]),
            Projection::KEY_ONLY,
            vec![
                lookup(),
                query_group(),
                term_seek(),
                posting(),
                probe(),
                posting(),
                probe(),
                emit(1),
            ],
            1 + 5 + 4 + (1 + 2) + (1 + 2) + (3 + 1),
            &[2],
        ),
    );
    // [Exists(price, I64), NOT Eq(kind, book), Range(price, ..=5)],
    // ID_ONLY: candidate-major probing; a candidate failing the negated
    // second literal is never probed by the third: 1 (fails NOT) -> 13; 2
    // (fails NOT) -> 16; 3 (passes NOT, passes Range) -> 21; emit -> 24.
    assert_query_on_both(
        &engine,
        ns,
        &model,
        &QueryVector::new(
            "price exists AND NOT book AND price <= 5",
            Query::all(vec![
                Literal::of(exists("price", TypeTag::I64)),
                Literal::not(eq("kind", Value::str("book"))),
                Literal::of(range(
                    "price",
                    Bound::Unbounded,
                    Bound::Included(Value::I64(5)),
                )),
            ]),
            Projection::ID_ONLY,
            vec![
                lookup(),
                query_group(),
                term_seek(),
                posting(),
                probe(),
                posting(),
                probe(),
                posting(),
                probe(),
                probe(),
                emit(0),
            ],
            1 + 5 + 4 + (1 + 2) + (1 + 2) + (1 + 2 + 2) + 3,
            &[3],
        ),
    );
    // A negated first literal followed by a positive one: every row is
    // visited and probed by both while it passes: [NOT Eq(kind, pen),
    // Eq(price, 20)]: 1 (passes NOT, fails Eq) -> 1 + 2 + 2; 2 (passes
    // both) -> 1 + 2 + 2; emit; then 3 (fails NOT) -> 1 + 2.
    assert_query_on_both(
        &engine,
        ns,
        &model,
        &QueryVector::new(
            "NOT pen AND price 20",
            Query::all(vec![
                Literal::not(eq("kind", Value::str("pen"))),
                Literal::of(eq("price", Value::I64(20))),
            ]),
            Projection::ID_ONLY,
            vec![
                lookup(),
                query_group(),
                row_visit(),
                probe(),
                probe(),
                row_visit(),
                probe(),
                probe(),
                emit(0),
                row_visit(),
                probe(),
            ],
            1 + 5 + (1 + 2 + 2) + (1 + 2 + 2) + 3 + (1 + 2),
            &[2],
        ),
    );
}

#[test]
fn or_groups_open_in_order_and_merge_interleaved_heads() {
    let (engine, ns, model) = fixture();
    // G1 = Eq(kind, book) {1, 2}; G2 = Range(price, 5..=10): terms 5 {3},
    // 10 {1} (20 terminates unpaid). Open in order: G1 QueryGroup 5 +
    // TermSeek 4 -> 10; G2 QueryGroup + TermSeek + TermVisit 2 -> 25. First
    // heads in order: G1 PostingEntry (1) -> 26; G2 PostingEntry (1) -> 27.
    // Merge: m = 1, emit -> 30, advance both: G1 (2) -> 31, G2 (3) -> 32;
    // m = 2, emit -> 35, G1 exhausted (no charge); m = 3, emit -> 38, G2
    // empty. Record 1 matched both groups and is emitted once.
    let query = Query {
        groups: vec![
            vec![Literal::of(eq("kind", Value::str("book")))],
            vec![Literal::of(range(
                "price",
                Bound::Included(Value::I64(5)),
                Bound::Included(Value::I64(10)),
            ))],
        ],
    };
    let open = || {
        vec![
            lookup(),
            query_group(),
            term_seek(),
            query_group(),
            term_seek(),
            term_visit(),
            term_visit(),
        ]
    };
    let mut full = open();
    full.extend([
        posting(),
        posting(),
        emit(0),
        posting(),
        posting(),
        emit(0),
        emit(0),
    ]);
    assert_query_on_both(
        &engine,
        ns,
        &model,
        &QueryVector::new(
            "book OR price 5..=10",
            query.clone(),
            Projection::ID_ONLY,
            full,
            (1 + 5 + 4 + 5 + 4 + 3 + 3) + 1 + 1 + 3 + 1 + 1 + 3 + 3,
            &[1, 2, 3],
        ),
    );
    // limit 2: after row 2 only G1 is advanced (exhausted, unpaid); G2's
    // head 3 is beyond the page: next after 2 at 35.
    let mut two = open();
    two.extend([posting(), posting(), emit(0), posting(), posting(), emit(0)]);
    assert_query_on_both(
        &engine,
        ns,
        &model,
        &QueryVector::new(
            "book OR price 5..=10 limit 2",
            query.clone(),
            Projection::ID_ONLY,
            two,
            (1 + 5 + 4 + 5 + 4 + 3 + 3) + 1 + 1 + 3 + 1 + 1 + 3,
            &[1, 2],
        )
        .limit(2)
        .next(2),
    );
    // The continuation after 2: both groups are opened again (the run terms
    // are visited regardless of the cursor); G1's posting after 2 is empty
    // (unpaid); G2 yields 3: PostingEntry -> 26; emit -> 29; next None.
    let snapshot = engine.head_snapshot();
    let first = snapshot
        .query(
            ns,
            &query,
            &Projection::ID_ONLY,
            2,
            None,
            Budget::unlimited(),
        )
        .expect("applies");
    let cursor = first.value.next.expect("row 3 follows");
    let mut rest = open();
    rest.extend([posting(), emit(0)]);
    assert_query_vector(
        &model,
        &QueryVector::new(
            "book OR price 5..=10 continuation",
            query.clone(),
            Projection::ID_ONLY,
            rest,
            (1 + 5 + 4 + 5 + 4 + 3 + 3) + 1 + 3,
            &[3],
        )
        .limit(2)
        .after(rid(2)),
        |budget| snapshot.query(ns, &query, &Projection::ID_ONLY, 2, Some(&cursor), budget),
    );
    // Three groups whose heads interleave: [price 20] {2}, [kind pen] {3},
    // [price 10] {1}: open 3 x (5 + 4) -> 28; heads 2, 3, 1 -> 31; emit 1,
    // advance G3 (exhausted); emit 2, advance G1; emit 3, advance G2: 40.
    assert_query_on_both(
        &engine,
        ns,
        &model,
        &QueryVector::new(
            "price 20 OR pen OR price 10",
            Query {
                groups: vec![
                    vec![Literal::of(eq("price", Value::I64(20)))],
                    vec![Literal::of(eq("kind", Value::str("pen")))],
                    vec![Literal::of(eq("price", Value::I64(10)))],
                ],
            },
            Projection::ID_ONLY,
            vec![
                lookup(),
                query_group(),
                term_seek(),
                query_group(),
                term_seek(),
                query_group(),
                term_seek(),
                posting(),
                posting(),
                posting(),
                emit(0),
                emit(0),
                emit(0),
            ],
            1 + (5 + 4) + (5 + 4) + (5 + 4) + 1 + 1 + 1 + 3 + 3 + 3,
            &[1, 2, 3],
        ),
    );
    // An empty group ORed with anything is the row map: [[], [Eq(kind,
    // book)]]: G1 QueryGroup -> 6; G2 QueryGroup + TermSeek -> 15; heads
    // G1 RowVisit (1) -> 16, G2 PostingEntry (1) -> 17; emit 1 -> 20,
    // advance G1 (2) -> 21, G2 (2) -> 22; emit 2 -> 25, advance G1 (3) ->
    // 26, G2 exhausted; emit 3 -> 29, G1 exhausted.
    assert_query_on_both(
        &engine,
        ns,
        &model,
        &QueryVector::new(
            "all OR book",
            Query {
                groups: vec![
                    Vec::new(),
                    vec![Literal::of(eq("kind", Value::str("book")))],
                ],
            },
            Projection::ID_ONLY,
            vec![
                lookup(),
                query_group(),
                query_group(),
                term_seek(),
                row_visit(),
                posting(),
                emit(0),
                row_visit(),
                posting(),
                emit(0),
                row_visit(),
                emit(0),
            ],
            1 + 5 + (5 + 4) + 1 + 1 + 3 + 1 + 1 + 3 + 1 + 3,
            &[1, 2, 3],
        ),
    );
}

#[test]
fn count_vectors_walk_every_head_without_row_charges() {
    let (engine, ns, model) = fixture();
    let snapshot = engine.head_snapshot();
    let branch = begin(&engine, None);
    // (label, query, groups, total): steps 1-3 as a query, then the merge
    // loop advances every head to exhaustion with no ResultRow or ReadByte.
    let vectors: Vec<(&str, Query, Vec<Group>, u64)> = vec![
        (
            // The worked vector: RowVisit 1, 2, 3.
            "match-all",
            Query::match_all(),
            vec![
                lookup(),
                query_group(),
                row_visit(),
                row_visit(),
                row_visit(),
            ],
            1 + 5 + 1 + 1 + 1,
        ),
        (
            // The worked vector: PostingEntry 1, 2, 3.
            "Exists(price, I64)",
            Query::term(exists("price", TypeTag::I64)),
            vec![
                lookup(),
                query_group(),
                term_seek(),
                posting(),
                posting(),
                posting(),
            ],
            1 + 5 + 4 + 1 + 1 + 1,
        ),
        (
            "Eq(kind, ink)",
            Query::term(eq("kind", Value::str("ink"))),
            vec![lookup(), query_group(), term_seek()],
            1 + 5 + 4,
        ),
        (
            // Every row is visited and probed; the one match costs nothing more.
            "NOT StartsWith(kind, bo)",
            Query::all(vec![Literal::not(starts_with("kind", "bo"))]),
            vec![
                lookup(),
                query_group(),
                row_visit(),
                probe(),
                row_visit(),
                probe(),
                row_visit(),
                probe(),
            ],
            1 + 5 + (1 + 2) + (1 + 2) + (1 + 2),
        ),
        (
            // book OR price 5..=10: open both (25); heads 1, 1 (27); advance
            // both past 1 (29); G1 and G2 then exhaust unpaid. 1 is counted
            // once.
            "book OR price 5..=10",
            Query {
                groups: vec![
                    vec![Literal::of(eq("kind", Value::str("book")))],
                    vec![Literal::of(range(
                        "price",
                        Bound::Included(Value::I64(5)),
                        Bound::Included(Value::I64(10)),
                    ))],
                ],
            },
            vec![
                lookup(),
                query_group(),
                term_seek(),
                query_group(),
                term_seek(),
                term_visit(),
                term_visit(),
                posting(),
                posting(),
                posting(),
                posting(),
            ],
            (1 + 5 + 4 + 5 + 4 + 3 + 3) + 1 + 1 + 1 + 1,
        ),
        (
            // book AND price 20: PostingEntry + probe for 1 (fails), 2 (passes).
            "book AND price 20",
            Query::all(vec![
                Literal::of(eq("kind", Value::str("book"))),
                Literal::of(eq("price", Value::I64(20))),
            ]),
            vec![
                lookup(),
                query_group(),
                term_seek(),
                posting(),
                probe(),
                posting(),
                probe(),
            ],
            1 + 5 + 4 + (1 + 2) + (1 + 2),
        ),
    ];
    for (label, query, groups, total) in &vectors {
        let expected = model_count(&model, query);
        assert_read_vector(
            &format!("snapshot count {label}"),
            groups,
            *total,
            &expected,
            |budget| snapshot.count(ns, query, budget),
        );
        assert_read_vector(
            &format!("branch count {label}"),
            groups,
            *total,
            &expected,
            |budget| branch.count(ns, query, budget),
        );
    }
    assert_eq!(model_count(&model, &Query::match_all()), 3);
    assert_eq!(
        model_count(&model, &Query::term(eq("kind", Value::str("ink")))),
        0
    );
    assert_eq!(
        model_count(
            &model,
            &Query::all(vec![Literal::not(starts_with("kind", "bo"))])
        ),
        1
    );
    assert!(branch.receipts().is_empty(), "counts are never recorded");
}

#[test]
fn typed_reads_on_an_unknown_namespace_charge_the_lookup_only() {
    let (engine, _ns, _model) = fixture();
    let snapshot = engine.head_snapshot();
    let unknown = vocab::ns(9);
    let miss = Outcome::Rejected(Rejection::UnknownNamespace);
    let query = Query::term(eq("kind", Value::str("book")));
    for budget in [Budget::limit(1), Budget::limit(500), Budget::unlimited()] {
        let rejected = snapshot
            .get_record(unknown, &key("a"), &Projection::ALL, budget)
            .expect_err("ordinal 9 does not exist");
        assert_eq!(rejected.reason, Rejection::UnknownNamespace);
        assert_eq!(rejected.receipt, read_receipt(1, miss), "{budget:?}");
        let rejected = snapshot
            .query(unknown, &query, &Projection::ALL, 10, None, budget)
            .expect_err("ordinal 9 does not exist");
        assert_eq!(rejected.receipt, read_receipt(1, miss), "{budget:?}");
        let rejected = snapshot
            .count(unknown, &query, budget)
            .expect_err("ordinal 9 does not exist");
        assert_eq!(rejected.receipt, read_receipt(1, miss), "{budget:?}");
    }
    // Budget 0: the lookup itself does not fit.
    for rejected in [
        snapshot
            .get_record(unknown, &key("a"), &Projection::ALL, Budget::limit(0))
            .expect_err("nothing fits 0"),
        snapshot
            .query(
                unknown,
                &query,
                &Projection::ALL,
                10,
                None,
                Budget::limit(0).traced(),
            )
            .expect_err("nothing fits 0"),
        snapshot
            .count(unknown, &query, Budget::limit(0))
            .expect_err("nothing fits 0"),
    ] {
        assert_eq!(rejected.reason, Rejection::OutOfBudget);
        assert_eq!(rejected.receipt.spent, Cost::ZERO);
        assert_eq!(rejected.receipt.kind, ReceiptKind::Read);
    }
}

#[test]
fn block_budget_vector_for_typed_ops_admits_charges_and_records_as_specified() {
    let (mut engine, ns, mut model) = fixture();
    let mut branch = begin(&engine, Some(303));
    let cells = fixture_cells("book", 10);
    let exceeds = |remaining: u64| Rejection::ExceedsBlockBudget {
        remaining: Cost::new(remaining),
    };

    // op1: create d (no new term) = 199 <= 303 -> Applied; remaining 104.
    let op1 = branch
        .execute(
            ALICE,
            create_record(ns, "d", cells.clone()),
            Budget::unlimited(),
        )
        .expect("199 fits 303");
    assert_eq!(op1.receipt, write_receipt(199, Outcome::Applied));
    model.create("d", &cells);
    assert_eq!(branch.remaining(), Cost::new(104));

    // op2: a limited budget above the remaining is refused at admission:
    // spent 0, nothing recorded, nothing charged; 105 too, 104 is admitted.
    for limit in [150, 105] {
        let op2 = branch
            .execute(
                ALICE,
                create_record(ns, "e", cells.clone()),
                Budget::limit(limit),
            )
            .expect_err("above the remaining 104");
        assert_eq!(op2.reason, exceeds(104));
        assert_eq!(
            op2.receipt,
            write_receipt(0, Outcome::Rejected(exceeds(104)))
        );
    }
    assert_eq!(
        branch.receipts().len(),
        1,
        "admission rejections are not recorded"
    );
    assert_eq!(branch.remaining(), Cost::new(104));

    // op3: create e with 104: lookup 1, PointRead 4; the 135-unit record
    // group does not fit the 100 left -> OutOfBudget at 4, recorded,
    // draining the block.
    let op3 = branch
        .execute(
            ALICE,
            create_record(ns, "e", cells.clone()),
            Budget::limit(104),
        )
        .expect_err("139 > 104");
    assert_eq!(op3.receipt, write_receipt(4, OUT_OF_BUDGET));
    assert_eq!(branch.remaining(), Cost::new(100));

    // op4: delete a, unlimited, against the remaining 100: 1, 4, {20 + 16 +
    // RootUpdate 6} 46, then {IndexRemove 6 + RootUpdate 4} one at a time
    // 56, 66, 76, 86, 96; the sixth (106) does not fit -> OutOfBudget at 96;
    // a stays live with every term.
    let op4 = branch
        .execute(ALICE, delete_record(ns, "a"), Budget::unlimited().traced())
        .expect_err("106 > 100");
    assert_eq!(
        op4.receipt,
        traced(
            write_receipt(96, OUT_OF_BUDGET),
            ledger(&[
                (OperationClass::DirectoryLookup, 1, 1, 1),
                (OperationClass::PointRead, 1, 3, 3),
                (OperationClass::RecordDelete, 1, 20, 20),
                (OperationClass::CellRemove, 4, 4, 16),
                (OperationClass::RootUpdate, 3 + 5 * 2, 2, 26),
                (OperationClass::IndexRemove, 5, 6, 30),
            ])
        )
    );
    assert_eq!(branch.remaining(), Cost::new(4));
    assert_eq!(observe(&branch, ns), modelled(&model));

    // op5, op6: reads are priced but never deducted or recorded. get_record
    // b ALL = 54; count match-all over 4 rows = 1 + 5 + 4 = 10.
    let op5 = branch
        .get_record(ns, &key("b"), &Projection::ALL, Budget::unlimited())
        .expect("applies");
    assert_eq!(op5.receipt, read_receipt(54, Outcome::Applied));
    assert_eq!(op5.value, model.row("b", &Projection::ALL));
    let op6 = branch
        .count(ns, &Query::match_all(), Budget::unlimited())
        .expect("applies");
    assert_eq!(op6.receipt, read_receipt(1 + 5 + 4, Outcome::Applied));
    assert_eq!(op6.value, 4);
    assert_eq!(branch.remaining(), Cost::new(4));
    assert_eq!(branch.receipts().len(), 3);

    // Estimates obey the same admission and remaining: 5 > 4 is refused;
    // unlimited runs into the remaining 4 after the PointRead.
    let estimate = branch
        .estimate(
            ALICE,
            create_record(ns, "e", cells.clone()),
            Budget::limit(5),
        )
        .expect_err("5 > 4");
    assert_eq!(estimate.reason, exceeds(4));
    let estimate = branch
        .estimate(
            ALICE,
            create_record(ns, "e", cells.clone()),
            Budget::unlimited(),
        )
        .expect_err("139 > 4");
    assert_eq!(estimate.receipt, write_receipt(4, OUT_OF_BUDGET));
    assert_eq!(branch.receipts().len(), 3, "estimates are not recorded");
    assert_eq!(branch.remaining(), Cost::new(4));

    // op7: delete a with exactly the remaining 4: admitted; lookup and
    // PointRead fit, the delete group does not -> OutOfBudget at 4.
    let op7 = branch
        .execute(ALICE, delete_record(ns, "a"), Budget::limit(4))
        .expect_err("46 > 4");
    assert_eq!(op7.receipt, write_receipt(4, OUT_OF_BUDGET));
    assert_eq!(branch.remaining(), Cost::ZERO);

    // op8: with 0 remaining the lookup itself does not fit: spent 0, still
    // recorded. A limit of 1 is refused at admission; a limit of 0 is
    // admitted and fails the same way.
    let op8 = branch
        .execute(
            ALICE,
            create_record(ns, "e", cells.clone()),
            Budget::unlimited(),
        )
        .expect_err("nothing fits 0");
    assert_eq!(op8.receipt, write_receipt(0, OUT_OF_BUDGET));
    let op9 = branch
        .execute(
            ALICE,
            create_record(ns, "e", cells.clone()),
            Budget::limit(1),
        )
        .expect_err("1 > 0");
    assert_eq!(op9.reason, exceeds(0));
    let op10 = branch
        .execute(ALICE, create_record(ns, "e", cells), Budget::limit(0))
        .expect_err("nothing fits 0");
    assert_eq!(op10.receipt, write_receipt(0, OUT_OF_BUDGET));
    // Reads are unaffected by the exhausted block.
    assert_eq!(
        branch
            .get_record(ns, &key("d"), &Projection::KEY_ONLY, Budget::unlimited())
            .expect("applies")
            .receipt,
        read_receipt(1 + 3 + (3 + 1), Outcome::Applied)
    );

    // 199 + 4 + 96 + 4 + 0 + 0 = 303: the rejected ops drained the block.
    let sealed = branch.seal();
    assert_eq!(sealed.spent(), Cost::new(303));
    let recorded: Vec<Receipt> = sealed.receipts().iter().map(without_ledger).collect();
    assert_eq!(
        recorded,
        vec![
            write_receipt(199, Outcome::Applied),
            write_receipt(4, OUT_OF_BUDGET),
            write_receipt(96, OUT_OF_BUDGET),
            write_receipt(4, OUT_OF_BUDGET),
            write_receipt(0, OUT_OF_BUDGET),
            write_receipt(0, OUT_OF_BUDGET),
        ]
    );
    assert_eq!(
        sealed.receipts()[2].ledger,
        op4.receipt.ledger,
        "the traced receipt is recorded with its ledger"
    );
    let published = engine.publish(&sealed).expect("publish on snapshot 1");
    let head = begin(&engine, None);
    assert_eq!(head.base(), published);
    assert_eq!(observe(&head, ns), modelled(&model), "only d was added");
}

#[test]
fn execute_host_receipts_are_identical_to_execute_for_the_same_op() {
    let (engine, ns, _model) = fixture();
    let worked_patch = patch_record(
        ns,
        "a",
        vec![cell("price", attr(Value::U64(10)))],
        names(&["note", "missing"]),
    );
    // (label, op, hand-derived total): create d 199, the worked patch 183,
    // delete c 106.
    let ops = [
        (
            "create d",
            create_record(ns, "d", fixture_cells("book", 10)),
            199,
        ),
        ("patch a", worked_patch, 183),
        ("delete c", delete_record(ns, "c"), 106),
    ];
    for (label, op, total) in &ops {
        let budgets = [
            Budget::unlimited(),
            Budget::unlimited().traced(),
            Budget::limit(*total),
            Budget::limit(total - 1),
            Budget::limit(total - 1).traced(),
            Budget::limit(4),
            Budget::limit(3),
            Budget::limit(0),
        ];
        for budget in budgets {
            let mut owner = begin(&engine, Some(500));
            let mut host = begin(&engine, Some(500));
            let estimated = canonical(&host.estimate_host(op.clone(), budget));
            let by_owner = canonical(&owner.execute(ALICE, op.clone(), budget));
            let by_host = canonical(&host.execute_host(op.clone(), budget));
            assert_eq!(by_host, by_owner, "{label} under {budget:?}");
            assert_eq!(
                estimated, by_host,
                "{label}: estimate_host under {budget:?}"
            );
            assert_eq!(
                owner.receipts(),
                host.receipts(),
                "{label} under {budget:?}"
            );
            assert_eq!(owner.spent(), host.spent(), "{label} under {budget:?}");
            assert_eq!(
                observe(&owner, ns),
                observe(&host, ns),
                "{label} under {budget:?}"
            );
            // The applied case really applied on the host branch.
            if budget
                .limit_units()
                .is_none_or(|limit| limit.units() >= *total)
            {
                let recorded: Vec<Receipt> = host.receipts().iter().map(without_ledger).collect();
                assert_eq!(
                    recorded,
                    vec![write_receipt(*total, Outcome::Applied)],
                    "{label} under {budget:?}"
                );
            }
        }
    }

    // The host path skips the owner check: BOB is refused after the lookup,
    // the host is not.
    let mut branch = begin(&engine, None);
    let rejected = branch
        .execute(
            BOB,
            create_record(ns, "d", fixture_cells("book", 10)),
            Budget::unlimited(),
        )
        .expect_err("BOB is not the owner");
    assert_eq!(
        rejected.receipt,
        write_receipt(1, Outcome::Rejected(Rejection::Unauthorized))
    );
    let executed = branch
        .execute_host(
            create_record(ns, "d", fixture_cells("book", 10)),
            Budget::unlimited(),
        )
        .expect("the host needs no ownership");
    assert_eq!(executed.effect, Effect::RecordCreated { id: rid(4) });
    assert_eq!(executed.receipt, write_receipt(199, Outcome::Applied));
    // An unknown namespace still costs the host the lookup.
    let rejected = branch
        .execute_host(
            create_record(vocab::ns(9), "d", Vec::new()),
            Budget::unlimited(),
        )
        .expect_err("ordinal 9 does not exist");
    assert_eq!(
        rejected.receipt,
        write_receipt(1, Outcome::Rejected(Rejection::UnknownNamespace))
    );
    assert_eq!(branch.spent(), Cost::new(1 + 199 + 1));

    // A sequence on both paths, estimated first on the host path. After a
    // is gone price=10 is new again (229); NotFound on the host path is
    // charged and recorded the same way.
    let sequence: [(WriteOp, Canonical); 4] = [
        (
            delete_record(ns, "a"),
            Ok((
                Effect::RecordDeleted { id: rid(1) },
                write_receipt(106, Outcome::Applied),
            )),
        ),
        (
            create_record(ns, "d", fixture_cells("book", 10)),
            Ok((
                Effect::RecordCreated { id: rid(4) },
                write_receipt(229, Outcome::Applied),
            )),
        ),
        (
            patch_record(
                ns,
                "b",
                vec![cell("price", attr(Value::U64(20)))],
                Vec::new(),
            ),
            Ok((
                Effect::RecordPatched { id: rid(2) },
                write_receipt(182, Outcome::Applied),
            )),
        ),
        (
            delete_record(ns, "a"),
            Err((
                Rejection::NotFound,
                write_receipt(4, Outcome::Rejected(Rejection::NotFound)),
            )),
        ),
    ];
    let mut owner = begin(&engine, None);
    let mut host = begin(&engine, None);
    for (op, expected) in sequence {
        let estimated = host.estimate_host(op.clone(), Budget::unlimited());
        let by_host = host.execute_host(op.clone(), Budget::unlimited());
        assert_eq!(estimated, by_host, "estimate_host == execute_host");
        let by_owner = owner.execute(ALICE, op, Budget::unlimited());
        assert_eq!(canonical(&by_host), canonical(&by_owner));
        assert_eq!(canonical(&by_host), expected);
    }
    assert_eq!(host.receipts(), owner.receipts());
    assert_eq!(host.spent(), Cost::new(106 + 229 + 182 + 4));
    assert_eq!(observe(&host, ns), observe(&owner, ns));

    // Anything but a typed record op is refused at admission: spent 0, a
    // write receipt with an empty ledger, nothing recorded.
    let not_host = Rejection::InvalidArgument(ArgumentFault::NotHostOperation);
    let raw_ops = [
        create_namespace("test"),
        WriteOp::TransferNamespace {
            namespace: ns,
            expected_revision: arkiv_db::Revision(0),
            new_owner: BOB,
        },
        WriteOp::Put {
            namespace: ns,
            key: b"k".to_vec(),
            value: b"v".to_vec(),
        },
        WriteOp::Delete {
            namespace: ns,
            key: b"k".to_vec(),
        },
        // The restriction is decided before the namespace is looked up.
        WriteOp::Put {
            namespace: vocab::ns(9),
            key: b"k".to_vec(),
            value: b"v".to_vec(),
        },
    ];
    let mut branch = begin(&engine, Some(10));
    for op in raw_ops {
        for budget in [Budget::unlimited(), Budget::limit(1_000), Budget::limit(0)] {
            let rejected = branch
                .execute_host(op.clone(), budget.traced())
                .expect_err("not a host operation");
            assert_eq!(rejected.reason, not_host, "{op:?}");
            assert_eq!(
                rejected.receipt,
                traced(write_receipt(0, Outcome::Rejected(not_host)), ledger(&[])),
                "{op:?} under {budget:?}"
            );
            let estimated = branch
                .estimate_host(op.clone(), budget)
                .expect_err("not a host operation");
            assert_eq!(
                estimated.receipt,
                write_receipt(0, Outcome::Rejected(not_host))
            );
        }
    }
    // Record ops on the host path go on to the argument check.
    let unindexable = Rejection::InvalidArgument(ArgumentFault::BytesNotIndexable);
    let rejected = branch
        .execute_host(
            create_record(ns, "d", vec![cell("v", attr(Value::bytes(b"x")))]),
            Budget::limit(5),
        )
        .expect_err("bytes cannot be an attribute");
    assert_eq!(
        rejected.receipt,
        write_receipt(0, Outcome::Rejected(unindexable))
    );
    assert!(
        branch.receipts().is_empty(),
        "admission rejections are not recorded"
    );
    assert_eq!(branch.spent(), Cost::ZERO);
    assert_eq!(branch.namespace_count(), 1);
    let descriptor = branch
        .descriptor(ns, Budget::unlimited())
        .expect("exists")
        .value;
    assert_eq!(
        (descriptor.owner, descriptor.revision),
        (ALICE, arkiv_db::Revision(0))
    );
    // The host-path restriction is checked before the block budget: a
    // 1000-unit raw op on a 10-unit block is NotHostOperation, not
    // ExceedsBlockBudget; a typed op with the same budget is.
    let rejected = branch
        .execute_host(create_record(ns, "d", Vec::new()), Budget::limit(1_000))
        .expect_err("1000 > 10");
    assert_eq!(
        rejected.reason,
        Rejection::ExceedsBlockBudget {
            remaining: Cost::new(10)
        }
    );
}

#[test]
fn estimate_matches_execute_for_a_typed_operation_script() {
    let (engine, ns, _model) = fixture();
    let mut branch = begin(&engine, Some(947));
    let unauthorized = Outcome::Rejected(Rejection::Unauthorized);
    // Running block spent after each recorded step: 199, 331, 514, 620,
    // 624, 628, 787, (admission), 788, 789; remaining before the admission
    // step: 947 - 787 = 160.
    let exceeds = Rejection::ExceedsBlockBudget {
        remaining: Cost::new(160),
    };
    let script: Vec<ScriptStep<'_>> = vec![
        (
            // create d, no new term: 199.
            "create d",
            ALICE,
            create_record(ns, "d", fixture_cells("book", 10)),
            Budget::unlimited(),
            Ok((
                Effect::RecordCreated { id: rid(4) },
                write_receipt(199, Outcome::Applied),
            )),
        ),
        (
            // patch a price := I64(11): 1 + 3 + 78 + 10 + 40 = 132.
            "patch a value",
            ALICE,
            patch_record(
                ns,
                "a",
                vec![cell("price", attr(Value::I64(11)))],
                Vec::new(),
            ),
            Budget::limit(132),
            Ok((
                Effect::RecordPatched { id: rid(1) },
                write_receipt(132, Outcome::Applied),
            )),
        ),
        (
            // The worked patch on the patched a (price I64(11) held by a
            // alone): row n = 40, remove 2, insert 2 new terms: 183.
            "patch a type",
            ALICE,
            patch_record(
                ns,
                "a",
                vec![cell("price", attr(Value::U64(10)))],
                names(&["note", "missing"]),
            ),
            Budget::unlimited().traced(),
            Ok((
                Effect::RecordPatched { id: rid(1) },
                traced(
                    write_receipt(183, Outcome::Applied),
                    ledger(&[
                        (OperationClass::DirectoryLookup, 1, 1, 1),
                        (OperationClass::PointRead, 1, 3, 3),
                        (OperationClass::RecordPatch, 1, 10, 10),
                        (OperationClass::CellWrite, 1, 4, 4),
                        (OperationClass::CellByte, 13, 1, 13),
                        (OperationClass::CellRemove, 2, 4, 8),
                        (OperationClass::RowByte, 40, 1, 40),
                        (OperationClass::RootUpdate, 10, 2, 20),
                        (OperationClass::IndexRemove, 2, 6, 12),
                        (OperationClass::IndexInsert, 2, 6, 12),
                        (OperationClass::IndexTermCreate, 2, 30, 60),
                    ]),
                ),
            )),
        ),
        (
            // delete c: 106, exact budget.
            "delete c",
            ALICE,
            delete_record(ns, "c"),
            Budget::limit(106),
            Ok((
                Effect::RecordDeleted { id: rid(3) },
                write_receipt(106, Outcome::Applied),
            )),
        ),
        (
            "delete c again",
            ALICE,
            delete_record(ns, "c"),
            Budget::unlimited(),
            Err((
                Rejection::NotFound,
                write_receipt(4, Outcome::Rejected(Rejection::NotFound)),
            )),
        ),
        (
            "create a again",
            ALICE,
            create_record(ns, "a", Vec::new()),
            Budget::unlimited(),
            Err((
                Rejection::AlreadyExists,
                write_receipt(4, Outcome::Rejected(Rejection::AlreadyExists)),
            )),
        ),
        (
            // create e with a's cells under 160: 1, 4, 139, then $expiresAt
            // value (held) 149, existence 159, kind value would reach 169 ->
            // OutOfBudget at 159 with two inserts in the ledger.
            "create e tight",
            ALICE,
            create_record(ns, "e", fixture_cells("book", 10)),
            Budget::limit(160).traced(),
            Err((
                Rejection::OutOfBudget,
                traced(
                    write_receipt(159, OUT_OF_BUDGET),
                    ledger(&[
                        (OperationClass::DirectoryLookup, 1, 1, 1),
                        (OperationClass::PointRead, 1, 3, 3),
                        (OperationClass::RecordCreate, 1, 20, 20),
                        (OperationClass::CellWrite, 4, 4, 16),
                        (OperationClass::CellByte, 46, 1, 46),
                        (OperationClass::RowByte, 47, 1, 47),
                        (OperationClass::RootUpdate, 3 + 2 * 2, 2, 14),
                        (OperationClass::IndexInsert, 2, 6, 12),
                    ]),
                ),
            )),
        ),
        (
            "oversized",
            ALICE,
            create_record(ns, "e", fixture_cells("book", 10)),
            Budget::limit(161),
            Err((exceeds, write_receipt(0, Outcome::Rejected(exceeds)))),
        ),
        (
            "unauthorized",
            BOB,
            create_record(ns, "f", Vec::new()),
            Budget::unlimited(),
            Err((Rejection::Unauthorized, write_receipt(1, unauthorized))),
        ),
        (
            "unknown",
            ALICE,
            create_record(vocab::ns(9), "f", Vec::new()),
            Budget::unlimited(),
            Err((
                Rejection::UnknownNamespace,
                write_receipt(1, Outcome::Rejected(Rejection::UnknownNamespace)),
            )),
        ),
        (
            // An empty patch is golem-db's no-op: the directory lookup, the
            // key lookup and the patch base charge, nothing written.
            "empty patch",
            ALICE,
            patch_record(ns, "a", Vec::new(), Vec::new()),
            Budget::unlimited().traced(),
            Ok((
                Effect::RecordPatched { id: rid(1) },
                traced(
                    write_receipt(14, Outcome::Applied),
                    ledger(&[
                        (OperationClass::DirectoryLookup, 1, 1, 1),
                        (OperationClass::PointRead, 1, 3, 3),
                        (OperationClass::RecordPatch, 1, 10, 10),
                    ]),
                ),
            )),
        ),
        (
            "empty patch of a missing key",
            ALICE,
            patch_record(ns, "zz", Vec::new(), Vec::new()),
            Budget::unlimited(),
            Err((
                Rejection::NotFound,
                write_receipt(4, Outcome::Rejected(Rejection::NotFound)),
            )),
        ),
    ];
    assert_estimate_matches_execute(&mut branch, script);
    assert_eq!(branch.spent(), Cost::new(789 + 14 + 4));
    assert_eq!(
        branch.receipts().len(),
        11,
        "the admission rejection is not recorded"
    );
    assert_eq!(branch.index_consistent(ns), Some(true));
}

#[test]
fn typed_estimates_have_no_side_effects_and_consume_no_ids() {
    let (engine, ns, model) = fixture();
    let mut branch = begin(&engine, Some(1_000));
    let before = observe(&branch, ns);
    assert_eq!(before, modelled(&model));

    // create d 199, patch a value 132, delete a 106; each on scratch state.
    let first = branch
        .estimate(
            ALICE,
            create_record(ns, "d", fixture_cells("book", 10)),
            Budget::unlimited(),
        )
        .expect("applies");
    assert_eq!(first.effect, Effect::RecordCreated { id: rid(4) });
    assert_eq!(first.receipt, write_receipt(199, Outcome::Applied));
    let second = branch
        .estimate(
            ALICE,
            create_record(ns, "d", fixture_cells("book", 10)),
            Budget::unlimited(),
        )
        .expect("applies");
    assert_eq!(
        second.effect,
        Effect::RecordCreated { id: rid(4) },
        "no id was consumed"
    );
    let estimated = branch
        .estimate(
            ALICE,
            patch_record(
                ns,
                "a",
                vec![cell("price", attr(Value::I64(11)))],
                Vec::new(),
            ),
            Budget::unlimited(),
        )
        .expect("applies");
    assert_eq!(estimated.receipt, write_receipt(132, Outcome::Applied));
    let estimated = branch
        .estimate(ALICE, delete_record(ns, "a"), Budget::unlimited())
        .expect("applies");
    assert_eq!(estimated.receipt, write_receipt(106, Outcome::Applied));
    assert!(branch.receipts().is_empty());
    assert_eq!(branch.spent(), Cost::ZERO);
    assert_eq!(branch.remaining(), Cost::new(1_000));
    assert_eq!(observe(&branch, ns), before, "estimates leave no trace");
    // A branch cursor survives estimates (no mutation happened).
    let query = Query::match_all();
    let page = branch
        .query(
            ns,
            &query,
            &Projection::ID_ONLY,
            1,
            None,
            Budget::unlimited(),
        )
        .expect("applies");
    let cursor = page.value.next.expect("rows follow");
    branch
        .estimate(ALICE, delete_record(ns, "b"), Budget::unlimited())
        .expect("applies");
    let next = branch
        .query(
            ns,
            &query,
            &Projection::ID_ONLY,
            10,
            Some(&cursor),
            Budget::unlimited(),
        )
        .expect("the cursor is still bound");
    assert_eq!(ids_of(&next.value.rows), vec![2, 3]);

    // Executing then estimating: the estimate sees the executed state.
    let executed = branch
        .execute(
            ALICE,
            create_record(ns, "d", fixture_cells("book", 10)),
            Budget::unlimited(),
        )
        .expect("applies");
    assert_eq!(executed.effect, Effect::RecordCreated { id: rid(4) });
    let third = branch
        .estimate(
            ALICE,
            create_record(ns, "e", fixture_cells("book", 10)),
            Budget::unlimited(),
        )
        .expect("applies");
    assert_eq!(third.effect, Effect::RecordCreated { id: rid(5) });
    assert_eq!(branch.receipts().len(), 1);
    assert_eq!(branch.spent(), Cost::new(199));
}

#[test]
fn cost_overflow_on_typed_ops_under_a_uniform_maximal_schedule() {
    // The fixture is published under FIXTURE; a second block commits
    // uniform(7, u64::MAX) so that snapshot 2 prices every class at u64::MAX.
    let (mut engine, ns, model) = fixture();
    let second = publish_pricing(&mut engine, PriceSchedule::uniform(7, u64::MAX));
    let snapshot = engine.snapshot(second).expect("snapshot 2 exists");
    assert_eq!(snapshot.pricing().id(), 7);
    let maximal = |kind: ReceiptKind, outcome: Outcome| receipt(kind, 7, u64::MAX, outcome);

    // Writes on a branch of snapshot 2 (block budget None: remaining
    // u64::MAX). create d: DirectoryLookup 1 * u64::MAX fits exactly;
    // PointRead would make the running total overflow -> CostOverflow with
    // u64::MAX spent, recorded, nothing created.
    let mut branch = engine.begin(second, None).expect("snapshot 2 is the head");
    let rejected = branch
        .execute(
            ALICE,
            create_record(ns, "d", fixture_cells("book", 10)),
            Budget::unlimited(),
        )
        .expect_err("the second charge overflows");
    assert_eq!(rejected.reason, Rejection::CostOverflow);
    assert_eq!(rejected.receipt, maximal(ReceiptKind::Write, COST_OVERFLOW));
    assert_eq!(branch.spent(), Cost::new(u64::MAX));
    assert_eq!(branch.remaining(), Cost::ZERO);
    assert_eq!(branch.typed_record_count(ns), Some(3));
    // With 0 remaining the next typed op cannot afford its lookup.
    let rejected = branch
        .execute(ALICE, delete_record(ns, "a"), Budget::unlimited())
        .expect_err("nothing fits 0");
    assert_eq!(
        rejected.receipt,
        receipt(ReceiptKind::Write, 7, 0, OUT_OF_BUDGET)
    );
    assert_eq!(
        branch.receipts(),
        &[
            maximal(ReceiptKind::Write, COST_OVERFLOW),
            receipt(ReceiptKind::Write, 7, 0, OUT_OF_BUDGET),
        ]
    );

    // Reads at snapshot 2: the lookup fits an unlimited budget, the next
    // charge (PointRead or QueryGroup) overflows.
    let query = Query::term(eq("kind", Value::str("book")));
    let rejected = snapshot
        .get_record(ns, &key("a"), &Projection::ALL, Budget::unlimited())
        .expect_err("PointRead overflows");
    assert_eq!(rejected.reason, Rejection::CostOverflow);
    assert_eq!(rejected.receipt, maximal(ReceiptKind::Read, COST_OVERFLOW));
    let rejected = snapshot
        .query(
            ns,
            &query,
            &Projection::ALL,
            10,
            None,
            Budget::unlimited().traced(),
        )
        .expect_err("QueryGroup overflows");
    assert_eq!(
        rejected.receipt,
        traced(
            maximal(ReceiptKind::Read, COST_OVERFLOW),
            ledger(&[(OperationClass::DirectoryLookup, 1, u64::MAX, u64::MAX)])
        )
    );
    let rejected = snapshot
        .count(ns, &query, Budget::unlimited())
        .expect_err("QueryGroup overflows");
    assert_eq!(rejected.receipt, maximal(ReceiptKind::Read, COST_OVERFLOW));
    // One unit less than the first charge is OutOfBudget at 0.
    let rejected = snapshot
        .count(ns, &query, Budget::limit(u64::MAX - 1))
        .expect_err("one short of the first charge");
    assert_eq!(
        rejected.receipt,
        receipt(ReceiptKind::Read, 7, 0, OUT_OF_BUDGET)
    );
    // A single maximal charge that ends the operation fits: an unknown
    // namespace costs exactly u64::MAX.
    let rejected = snapshot
        .count(vocab::ns(9), &query, Budget::limit(u64::MAX))
        .expect_err("ordinal 9 does not exist");
    assert_eq!(
        rejected.receipt,
        maximal(
            ReceiptKind::Read,
            Outcome::Rejected(Rejection::UnknownNamespace)
        )
    );
    // Snapshot 1 still prices with FIXTURE: get_record a ALL = 54.
    let first = engine
        .snapshot(snapshot.base().expect("has a base"))
        .expect("snapshot 1");
    let row = first
        .get_record(ns, &key("a"), &Projection::ALL, Budget::unlimited())
        .expect("applies");
    assert_eq!(row.receipt, read_receipt(54, Outcome::Applied));
    assert_eq!(row.value, model.row("a", &Projection::ALL));
}

#[test]
fn cost_overflow_inside_a_group_charges_nothing_of_the_group() {
    // CellByte at u64::MAX, id 8: a create with any cell bytes overflows in
    // the record group, after the lookup and the PointRead.
    let (engine, ns) = bare_with(
        PriceSchedule::FIXTURE
            .with_price(OperationClass::CellByte, u64::MAX)
            .with_id(8),
        Limits::DEFAULT,
    );
    let mut branch = begin(&engine, None);
    let rejected = branch
        .execute(
            ALICE,
            create_record(ns, "a", fixture_cells("book", 10)),
            Budget::unlimited().traced(),
        )
        .expect_err("46 * u64::MAX overflows");
    assert_eq!(rejected.reason, Rejection::CostOverflow);
    assert_eq!(
        rejected.receipt,
        traced(
            receipt(ReceiptKind::Write, 8, 4, COST_OVERFLOW),
            ledger(&[
                (OperationClass::DirectoryLookup, 1, 1, 1),
                (OperationClass::PointRead, 1, 3, 3),
            ])
        )
    );
    assert_eq!(branch.typed_record_count(ns), Some(0));
    assert_eq!(branch.term_count(ns), Some(0));
    // One small cell, k = Bool(true): b = 1 + 1 = 2, and 2 * u64::MAX
    // already overflows the CellByte subtotal; still 4.
    let rejected = branch
        .execute(
            ALICE,
            create_record(ns, "a", vec![cell("k", attr(Value::Bool(true)))]),
            Budget::unlimited(),
        )
        .expect_err("the group total overflows");
    assert_eq!(
        rejected.receipt,
        receipt(ReceiptKind::Write, 8, 4, COST_OVERFLOW)
    );
    // No cells: 0 * u64::MAX = 0, so create z = 1 + 3 + (20 + RowByte 1 +
    // RootUpdate 3*2) = 31 applies.
    let executed = branch
        .execute(
            ALICE,
            create_record(ns, "z", Vec::new()),
            Budget::unlimited(),
        )
        .expect("no cell bytes are charged");
    assert_eq!(executed.effect, Effect::RecordCreated { id: rid(1) });
    assert_eq!(
        executed.receipt,
        receipt(ReceiptKind::Write, 8, 31, Outcome::Applied)
    );
    // get_record z ALL = 1 + 3 + (3 + 1) = 8 (ReadByte keeps its price).
    let row = branch
        .get_record(ns, &key("z"), &Projection::ALL, Budget::unlimited())
        .expect("applies");
    assert_eq!(
        row.receipt,
        receipt(ReceiptKind::Read, 8, 8, Outcome::Applied)
    );
    // 4 + 4 + 31 = 39.
    assert_eq!(branch.spent(), Cost::new(39));

    // IndexTermCreate at u64::MAX, id 9: the record group is charged (139),
    // the first index group {6 + u64::MAX + 4} overflows; no membership and
    // no row is stored.
    let (engine, ns) = bare_with(
        PriceSchedule::FIXTURE
            .with_price(OperationClass::IndexTermCreate, u64::MAX)
            .with_id(9),
        Limits::DEFAULT,
    );
    let mut branch = begin(&engine, None);
    let rejected = branch
        .execute(
            ALICE,
            create_record(ns, "a", fixture_cells("book", 10)),
            Budget::unlimited(),
        )
        .expect_err("6 + u64::MAX overflows");
    assert_eq!(
        rejected.receipt,
        receipt(ReceiptKind::Write, 9, 139, COST_OVERFLOW)
    );
    assert_eq!(branch.typed_record_count(ns), Some(0));
    assert_eq!(branch.term_count(ns), Some(0));
    assert_eq!(branch.index_consistent(ns), Some(true));
    // A create holding only fields never creates a term: 1 + 3 + (20 + 4 +
    // 7 + RowByte (1 + 7) + RootUpdate 3*2) = 49 applies under the same
    // schedule.
    let executed = branch
        .execute(
            ALICE,
            create_record(ns, "b", vec![cell("note", field(Value::bytes(b"xyz")))]),
            Budget::unlimited(),
        )
        .expect("fields are not indexed");
    assert_eq!(
        executed.receipt,
        receipt(ReceiptKind::Write, 9, 49, Outcome::Applied)
    );
}

#[test]
fn typed_pricing_is_pinned_to_the_base_and_applies_only_after_publication() {
    let (mut engine, ns, mut model) = fixture();
    let cells = fixture_cells("book", 10);
    let mut b1 = begin(&engine, None);
    assert_eq!(b1.pricing().id(), FIXTURE_ID);
    // create d = 199 under FIXTURE.
    let before = b1
        .execute(
            ALICE,
            create_record(ns, "d", cells.clone()),
            Budget::unlimited(),
        )
        .expect("applies");
    assert_eq!(before.receipt, write_receipt(199, Outcome::Applied));
    model.create("d", &cells);

    b1.set_pricing(PriceSchedule::uniform(2, 100))
        .expect("2 exceeds the committed id 1");
    // Still pinned to FIXTURE for writes, estimates and reads.
    let after = b1
        .execute(
            ALICE,
            create_record(ns, "e", cells.clone()),
            Budget::unlimited(),
        )
        .expect("applies");
    assert_eq!(after.receipt, write_receipt(199, Outcome::Applied));
    model.create("e", &cells);
    assert_eq!(b1.pricing().id(), FIXTURE_ID);
    let estimated = b1
        .estimate(ALICE, delete_record(ns, "a"), Budget::unlimited())
        .expect("applies");
    assert_eq!(estimated.receipt, write_receipt(106, Outcome::Applied));
    let row = b1
        .get_record(ns, &key("a"), &Projection::ALL, Budget::unlimited())
        .expect("applies");
    assert_eq!(row.receipt, read_receipt(54, Outcome::Applied));
    let query = Query::term(eq("kind", Value::str("book")));
    let page = b1
        .query(ns, &query, &Projection::ALL, 1, None, Budget::unlimited())
        .expect("applies");
    assert_eq!(page.receipt, read_receipt(62, Outcome::Applied));
    // count match-all over 5 rows: 1 + 5 + 5.
    let count = b1
        .count(ns, &Query::match_all(), Budget::unlimited())
        .expect("applies");
    assert_eq!(count.receipt, read_receipt(1 + 5 + 5, Outcome::Applied));
    assert_eq!(count.value, 5);
    assert_eq!(observe(&b1, ns), modelled(&model));

    let published = publish(&mut engine, b1);
    let snapshot = engine.snapshot(published).expect("snapshot 2 exists");
    assert_eq!(snapshot.pricing().id(), 2);
    // Reads at snapshot 2 use uniform 100. get_record a ALL: 1 + 1 + 1 + 47
    // = 50 units -> 5000.
    let row = snapshot
        .get_record(
            ns,
            &key("a"),
            &Projection::ALL,
            Budget::unlimited().traced(),
        )
        .expect("applies");
    assert_eq!(
        row.receipt,
        traced(
            receipt(ReceiptKind::Read, 2, 5_000, Outcome::Applied),
            ledger(&[
                (OperationClass::DirectoryLookup, 1, 100, 100),
                (OperationClass::PointRead, 1, 100, 100),
                (OperationClass::ResultRow, 1, 100, 100),
                (OperationClass::ReadByte, 47, 100, 4_700),
            ])
        )
    );
    assert_eq!(row.value, model.row("a", &Projection::ALL));
    // query Eq(kind, book) ALL limit 1: lookup 1 + group 1 + seek 1 +
    // PostingEntry 2 + ResultRow 1 + ReadByte 47 = 53 units -> 5300.
    let page = snapshot
        .query(ns, &query, &Projection::ALL, 1, None, Budget::unlimited())
        .expect("applies");
    assert_eq!(
        page.receipt,
        receipt(ReceiptKind::Read, 2, 5_300, Outcome::Applied)
    );
    assert_eq!(ids_of(&page.value.rows), vec![1]);
    // count match-all: 1 + 1 + 5 = 7 units -> 700.
    let count = snapshot
        .count(ns, &Query::match_all(), Budget::unlimited())
        .expect("applies");
    assert_eq!(
        count.receipt,
        receipt(ReceiptKind::Read, 2, 700, Outcome::Applied)
    );
    // One unit short of a charge is OutOfBudget at the previous point.
    let rejected = snapshot
        .count(ns, &Query::match_all(), Budget::limit(699))
        .expect_err("699 is one short");
    assert_eq!(
        rejected.receipt,
        receipt(ReceiptKind::Read, 2, 600, OUT_OF_BUDGET)
    );

    // A new branch on snapshot 2 writes under uniform 100, so each charged
    // count costs 100. create f with a's cells (every term held):
    // DirectoryLookup 1 + PointRead 1 + (RecordCreate 1 + CellWrite 4 +
    // CellByte 46 + RowByte 47 + RootUpdate 3) + 6 * (IndexInsert 1 +
    // RootUpdate 2) = 2 + 101 + 18 = 121 units -> 12100.
    let mut b2 = engine
        .begin(published, None)
        .expect("snapshot 2 is the head");
    assert_eq!(b2.pricing().id(), 2);
    let created = b2
        .execute(
            ALICE,
            create_record(ns, "f", cells.clone()),
            Budget::unlimited(),
        )
        .expect("applies");
    assert_eq!(created.effect, Effect::RecordCreated { id: rid(6) });
    assert_eq!(
        created.receipt,
        receipt(ReceiptKind::Write, 2, 12_100, Outcome::Applied)
    );
    let rejected = b2
        .execute(ALICE, create_record(ns, "g", cells), Budget::limit(12_099))
        .expect_err("one short of 12100");
    // 2 + 101 + 5 * 3 = 118 units charged before the sixth insert.
    assert_eq!(
        rejected.receipt,
        receipt(ReceiptKind::Write, 2, 11_800, OUT_OF_BUDGET)
    );
    // delete f: 1 + 1 + (RecordDelete 1 + CellRemove 4 + RootUpdate 3) + 6 *
    // (IndexRemove 1 + RootUpdate 2) = 2 + 8 + 18 = 28 units -> 2800.
    let deleted = b2
        .execute(ALICE, delete_record(ns, "f"), Budget::unlimited())
        .expect("applies");
    assert_eq!(
        deleted.receipt,
        receipt(ReceiptKind::Write, 2, 2_800, Outcome::Applied)
    );
    // patch a price := I64(11): 1 + 1 + (RecordPatch 1 + CellWrite 1 +
    // CellByte 13 + RowByte 47 + RootUpdate 2) + (IndexRemove 1 + RootUpdate
    // 2) + (IndexInsert 1 + IndexTermCreate 1 + RootUpdate 2) = 2 + 64 + 3 +
    // 4 = 73 units -> 7300 (value 11 is new).
    let patched = b2
        .execute(
            ALICE,
            patch_record(
                ns,
                "a",
                vec![cell("price", attr(Value::I64(11)))],
                Vec::new(),
            ),
            Budget::unlimited(),
        )
        .expect("applies");
    assert_eq!(
        patched.receipt,
        receipt(ReceiptKind::Write, 2, 7_300, Outcome::Applied)
    );

    // The earlier snapshot still prices with schedule 1 and never saw d.
    let first = engine
        .snapshot(snapshot.base().expect("has a base"))
        .expect("snapshot 1");
    assert_eq!(first.pricing().id(), FIXTURE_ID);
    let miss = first
        .get_record(ns, &key("d"), &Projection::ALL, Budget::unlimited())
        .expect("applies");
    assert_eq!(miss.receipt, read_receipt(4, Outcome::Applied));
    assert_eq!(miss.value, None);
}

/// A paged walk over a Range run charges the run's terms on every page: the
/// id-ordered merge consults every qualifying term's posting per page, so
/// three pages over the three-term (price, I64) run each pay TermVisit 3.
///
/// Fixture postings: 5 {3}, 10 {1}, 20 {2}. Page 1 (no cursor, limit 1):
/// 1 + 5 + 4 + TermVisit 9 = 19; PostingEntry for the head 1 -> 20; emit
/// ID_ONLY 3 -> 23; advancing past 1 reveals head 2: PostingEntry -> 24;
/// the page is full, next after 1. Page 2 (after 1): 19; the postings are
/// positioned after 1 (10 is exhausted, uncharged); head 2 -> 20; emit ->
/// 23; head 3 -> 24; next after 2. Page 3 (after 2): 19; head 3 -> 20; emit
/// -> 23; the run is exhausted, next None.
#[test]
fn range_walk_pages_revisit_every_term_of_the_run() {
    let (engine, ns, _model) = fixture();
    let snapshot = engine.head_snapshot();
    let query = Query::term(range(
        "price",
        Bound::Included(Value::I64(i64::MIN)),
        Bound::Unbounded,
    ));
    let mut cursor = None;
    for (expected_id, expected_cost) in [(1, 24), (2, 24), (3, 23)] {
        let page = snapshot
            .query(
                ns,
                &query,
                &Projection::ID_ONLY,
                1,
                cursor.as_ref(),
                Budget::unlimited().traced(),
            )
            .expect("the walk stays within budget");
        assert_eq!(ids_of(&page.value.rows), [expected_id]);
        let ledger = page.receipt.ledger.as_ref().expect("traced");
        let visits = ledger
            .entries
            .iter()
            .find(|entry| entry.class == OperationClass::TermVisit)
            .map(|entry| entry.count);
        assert_eq!(visits, Some(3), "every page visits the whole run");
        assert_eq!(
            without_ledger(&page.receipt),
            read_receipt(expected_cost, Outcome::Applied)
        );
        cursor = page.value.next;
    }
    assert!(cursor.is_none(), "the third page is terminal");
}

#[test]
fn planned_reads_match_the_plans_worked_vectors() {
    let (engine, ns, model) = fixture();
    let snapshot = engine.head_snapshot();
    let planned = Budget::unlimited().planned();
    let count = |query: &Query| {
        let metered = snapshot.count(ns, query, planned).expect("applies");
        (metered.value, metered.receipt.spent.units())
    };
    // The plan's vectors: match-all is the live row count, one posting its
    // stored size, a NotEq its existence posting less its value posting.
    assert_eq!(count(&Query::match_all()), (3, 1 + 5));
    assert_eq!(
        count(&Query::term(exists("price", TypeTag::I64))),
        (3, 1 + 5 + 4)
    );
    assert_eq!(
        count(&Query::term(not_eq("kind", Value::str("pen")))),
        (2, 1 + 5 + 4 + 4)
    );
    // The written charge of the match-all count is untouched: 9.
    assert_eq!(
        snapshot
            .count(ns, &Query::match_all(), Budget::unlimited())
            .expect("applies")
            .receipt
            .spent
            .units(),
        9
    );
    // A page: the one-entry posting of price 20 drives, kind = book follows
    // with one repositioning; 1 + 5 + two seeks 8 + PostingEntry 1 +
    // RecordProbe 2 + ResultRow 3 = 20, in either literal order.
    for literals in [
        vec![
            Literal::of(eq("kind", Value::str("book"))),
            Literal::of(eq("price", Value::I64(20))),
        ],
        vec![
            Literal::of(eq("price", Value::I64(20))),
            Literal::of(eq("kind", Value::str("book"))),
        ],
    ] {
        let query = Query::all(literals);
        let page = snapshot
            .query(ns, &query, &Projection::ID_ONLY, 10, None, planned)
            .expect("applies");
        assert_eq!(ids_of(&page.value.rows), vec![2]);
        assert_eq!(model.ids(&query), vec![rid(2)]);
        assert!(page.value.next.is_none());
        assert_eq!(page.receipt.spent.units(), 20);
    }
}
