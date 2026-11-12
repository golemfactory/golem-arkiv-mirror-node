//! Snapshot-pinned keyset pagination and cursor binding.
//!
//! A page walk reads every page from one pinned handle with the cursor the
//! previous page issued. The cursor is a record id, never a count, so a page
//! is the first `limit` matches with a larger id. Expected rows come from an
//! independent reference: a `BTreeMap` model of the records and a full-scan
//! evaluator of the query semantics. Expected costs are literals derived by
//! hand from the FIXTURE weights; the pages of the 64-record walk are
//! compared between runs, not priced.

use std::{collections::BTreeMap, ops::Bound};

use arkiv_db::{
    Branch, Budget, Cell, CellName, Cost, Cursor, Effect, Engine, EngineError, Limits, Literal,
    NamespaceId, OperationClass, Outcome, OwnerId, PriceSchedule, Projection, Query, QueryFault,
    QueryPage, ReadResult, ReceiptKind, RecordId, RecordKey, Rejected, Rejection, Row, SnapshotId,
    Term, TypeTag, Value, WriteOp,
};

use crate::support::{
    model::Model,
    vocab::{
        ALICE, BOB, FIXTURE_ID, add_namespace, apply, attr, begin, create_namespace, create_record,
        delete_record, field, key, ledger, name, ns, patch_record, publish, read_applied,
        read_receipt, receipt, rid, traced, transfer,
    },
    walk::{rows_of, walk},
};

const MAX_PAGE: usize = Limits::DEFAULT.max_page;
/// The page limits every full walk is run with.
const LIMITS: [usize; 4] = [1, 3, 7, MAX_PAGE];

/// The walk namespace holds records `r00`..`r63` (ids 1..=64). Record `i`
/// is a pen when `i % 9 == 4`, a lamp when `i % 9 == 8` and a book
/// otherwise: 7 pens, 7 lamps, 50 books.
const WALK_RECORDS: u64 = 64;
const BOOKS: usize = 50;

/// The cost namespace: six records with `kind: Str` and `price: I64`
/// attributes, created in this order (ids 1..=6). Books are the posting
/// `[1, 2, 4]`, lamps `[5, 6]`.
const COST_RECORDS: [(&str, &str, i64); 6] = [
    ("a", "book", 10),
    ("b", "book", 20),
    ("c", "pen", 5),
    ("d", "book", 30),
    ("e", "lamp", 7),
    ("f", "lamp", 8),
];

// Hand-computed read costs under `PriceSchedule::FIXTURE`: DirectoryLookup 1,
// ReadByte 1, QueryGroup 5, TermSeek 4, PostingEntry 1, RecordProbe 2,
// ResultRow 3. Every total is a literal with its sum beside it.

/// `Eq(kind, book)` over the posting `[1, 2, 4]`, `ID_ONLY`, limit 2, no
/// cursor: 1 + 5 + 4, PostingEntry 3 (rows 1 and 2 plus the head 4 beyond
/// the page, which makes `next` exact), ResultRow 2 = 19.
const EQ_PAGE_ONE: u64 = 19;
/// Page 2 after 2: 1 + 5 + 4, PostingEntry 1 (exactly the remaining entry;
/// the exhausted posting is not charged), ResultRow 1 = 14.
const EQ_PAGE_TWO: u64 = 14;
/// Limit 3 or more over the same posting: 1 + 5 + 4, PostingEntry 3,
/// ResultRow 3 = 22; the page is exactly full and `next` is `None`.
const EQ_FULL_PAGE: u64 = 22;
/// Limit 1 walk over the same posting: pages 1 and 2 charge PostingEntry 2
/// (the row and the head beyond it), ResultRow 1: 1 + 5 + 4 + 2 + 3 = 15.
const EQ_LIMIT_ONE_PAGE: u64 = 15;
/// A value term nobody holds: the seek only. 1 + 5 + 4 = 10.
const EQ_MISSING_TERM: u64 = 10;
/// Limit 2 with `Projection::ALL`: a cost record is key 1 + kind (4 + 4) +
/// price (5 + 8) = 22 bytes, so 19 + ReadByte 2 × 22 = 63.
const EQ_PAGE_ONE_ALL: u64 = 63;
/// `[Eq(kind, book)] OR [Eq(kind, lamp), Exists(price, I64)]`, `ID_ONLY`,
/// limit 2, no cursor. Group 1 (posting `[1, 2, 4]`) and group 2 (posting
/// `[5, 6]` probed by the existence literal): 1 + 2 × (5 + 4); heads:
/// PostingEntry 1 (id 1) and PostingEntry 1 + RecordProbe 1 (id 5); rows 1
/// and 2 (ResultRow 2) each advance group 1 (PostingEntry 2); group 2's head
/// 5 lies beyond the page end and is never advanced: 1 + 18 + 4 + 2 + 6 = 31.
const OR_PAGE_ONE: u64 = 31;
/// Page 2 after 2: heads 4 (PostingEntry 1) and 5 (PostingEntry 1 +
/// RecordProbe 1); row 4 exhausts group 1 for free; row 5 advances group 2
/// to 6 (PostingEntry 1 + RecordProbe 1): 1 + 18 + 3 + 4 + 6 = 32.
const OR_PAGE_TWO: u64 = 32;
/// Page 3 after 5: group 1's posting is empty past 5 (no entry charged);
/// group 2's head 6 (PostingEntry 1 + RecordProbe 1); row 6 exhausts it:
/// 1 + 18 + 1 + 2 + 3 = 25.
const OR_PAGE_THREE: u64 = 25;
/// A branch whose working state adds a fourth book (id 7): page 2 after 2
/// charges PostingEntry 2 (4 and 7) and ResultRow 2: 1 + 5 + 4 + 2 + 6 = 18.
const EQ_BRANCH_PAGE_TWO: u64 = 18;
/// `count(Eq(kind, book))` over the 50 books: 1 + 5 + 4 + PostingEntry 50 =
/// 60 (a count emits no row).
const COUNT_BOOKS: u64 = 60;
/// The same count after seven books were deleted at head: 1 + 5 + 4 + 43 = 53.
const COUNT_BOOKS_AFTER_DELETES: u64 = 53;

const CURSOR_MISMATCH: Outcome = Outcome::Rejected(Rejection::CursorMismatch);

/// A row under `Projection::ID_ONLY`.
fn id_row(ordinal: u64) -> Row {
    Row {
        id: rid(ordinal),
        key: None,
        cells: Vec::new(),
    }
}

/// `Eq(kind, value)` as a one-literal query.
fn kind_is(value: &str) -> Query {
    Query::term(Term::eq(name("kind"), Value::str(value)))
}

/// The primary query of every walk.
fn books() -> Query {
    kind_is("book")
}

/// `[Eq(kind, book)] OR [Eq(kind, lamp), Exists(price, I64)]`.
fn books_or_priced_lamps() -> Query {
    Query {
        groups: vec![
            vec![Literal::of(Term::eq(name("kind"), Value::str("book")))],
            vec![
                Literal::of(Term::eq(name("kind"), Value::str("lamp"))),
                Literal::of(Term::exists(name("price"), TypeTag::I64)),
            ],
        ],
    }
}

/// The cells of walk record `i` (its key is `r{i:02}`, its id `i + 1`):
/// `kind: Str` (book, pen or lamp), `price` (an `I64` attribute in
/// `-20..=80`, or a `U64` attribute when `i % 7 == 6`, so ranges over `I64`
/// skip it), `stock: U64(i / 4)` and, for even `i`, a `note` field.
fn walk_cells(i: u64) -> Vec<(CellName, Cell)> {
    let kind = match i % 9 {
        4 => "pen",
        8 => "lamp",
        _ => "book",
    };
    let price = if i % 7 == 6 {
        Value::U64(i)
    } else {
        let scrambled = i64::try_from((i * 37) % 101).expect("fits");
        Value::I64(scrambled - 20)
    };
    let mut cells = vec![
        (name("kind"), attr(Value::str(kind))),
        (name("price"), attr(price)),
        (name("stock"), attr(Value::U64(i / 4))),
    ];
    if i.is_multiple_of(2) {
        cells.push((name("note"), field(Value::str(&format!("n{i}")))));
    }
    cells
}

fn cost_cells(kind: &str, price: i64) -> Vec<(CellName, Cell)> {
    vec![
        (name("kind"), attr(Value::str(kind))),
        (name("price"), attr(Value::I64(price))),
    ]
}

/// A cost record under `Projection::ALL`.
fn cost_row(ordinal: u64, key_text: &str, kind: &str, price: i64) -> Row {
    Row {
        id: rid(ordinal),
        key: Some(key(key_text)),
        cells: cost_cells(kind, price),
    }
}

/// The eight query shapes every walk is run with, over the walk namespace.
fn shapes() -> Vec<(&'static str, Query)> {
    let kind = || name("kind");
    let price = || name("price");
    let stock = || name("stock");
    vec![
        ("Eq(kind, book)", books()),
        ("match-all", Query::match_all()),
        (
            "NOT Eq(kind, pen)",
            Query::all(vec![Literal::not(Term::eq(kind(), Value::str("pen")))]),
        ),
        (
            "Range(price, [0, +inf))",
            Query::term(Term::range(
                price(),
                Bound::Included(Value::I64(0)),
                Bound::Unbounded,
            )),
        ),
        (
            "StartsWith(kind, \"\")",
            Query::term(Term::starts_with(kind(), "")),
        ),
        (
            "[Eq(kind, pen)] OR [Range(stock, [3, 9))]",
            Query {
                groups: vec![
                    vec![Literal::of(Term::eq(kind(), Value::str("pen")))],
                    vec![Literal::of(Term::range(
                        stock(),
                        Bound::Included(Value::U64(3)),
                        Bound::Excluded(Value::U64(9)),
                    ))],
                ],
            },
        ),
        (
            "Exists(price, I64) AND NotEq(kind, book) AND NOT Range(stock, (-inf, 2])",
            Query::all(vec![
                Literal::of(Term::exists(price(), TypeTag::I64)),
                Literal::of(Term::not_eq(kind(), Value::str("book"))),
                Literal::not(Term::range(
                    stock(),
                    Bound::Unbounded,
                    Bound::Included(Value::U64(2)),
                )),
            ]),
        ),
        (
            "NotEq(kind, book)",
            Query::term(Term::not_eq(kind(), Value::str("book"))),
        ),
    ]
}

/// Unwraps a rejected read.
fn expect_rejected(result: ReadResult<QueryPage>, what: &str) -> Rejected {
    match result {
        Ok(page) => panic!(
            "{what}: expected a rejection, got a page of {} rows",
            page.value.rows.len()
        ),
        Err(rejected) => rejected,
    }
}

/// A rejection decided at admission: spent 0, a read receipt under the
/// reader's own schedule, and an empty ledger when one was requested.
fn assert_zero_cost_under(
    result: ReadResult<QueryPage>,
    reason: Rejection,
    schedule_id: u32,
    what: &str,
) {
    let mut rejected = expect_rejected(result, what);
    assert_eq!(rejected.reason, reason, "{what}");
    let ledger = rejected.receipt.ledger.take();
    assert!(
        ledger
            .as_ref()
            .is_none_or(|ledger| ledger.entries.is_empty()),
        "{what}: nothing is charged, got {ledger:?}"
    );
    assert_eq!(
        rejected.receipt,
        receipt(ReceiptKind::Read, schedule_id, 0, Outcome::Rejected(reason)),
        "{what}"
    );
}

fn assert_zero_cost(result: ReadResult<QueryPage>, reason: Rejection, what: &str) {
    assert_zero_cost_under(result, reason, FIXTURE_ID, what);
}

fn assert_mismatch(result: ReadResult<QueryPage>, what: &str) {
    assert_zero_cost(result, Rejection::CursorMismatch, what);
}

/// Snapshot 1 of a fresh engine: the walk namespace (ordinal 1) with its 64
/// records, the cost namespace (ordinal 2) with its six, both owned by
/// ALICE, priced with FIXTURE. `model` mirrors the walk namespace.
struct Fixture {
    engine: Engine,
    walk_ns: NamespaceId,
    cost_ns: NamespaceId,
    model: Model,
}

fn fixture() -> Fixture {
    let mut engine = Engine::new(PriceSchedule::FIXTURE);
    assert_eq!(engine.limits().max_page, MAX_PAGE);
    let mut branch = begin(&engine, None);
    let walk_ns = add_namespace(&mut branch, ALICE, "test");
    let cost_ns = add_namespace(&mut branch, ALICE, "test");
    let mut model = Model::new();
    for i in 0..WALK_RECORDS {
        let key = format!("r{i:02}");
        let cells = walk_cells(i);
        let expected = model.create(&key, &cells);
        let executed = apply(&mut branch, ALICE, create_record(walk_ns, &key, cells));
        assert_eq!(executed.effect, Effect::RecordCreated { id: expected });
    }
    for ((key_text, kind, price), ordinal) in COST_RECORDS.iter().zip(1u64..) {
        let op = create_record(cost_ns, key_text, cost_cells(kind, *price));
        let executed = apply(&mut branch, ALICE, op);
        assert_eq!(executed.effect, Effect::RecordCreated { id: rid(ordinal) });
    }
    let published = publish(&mut engine, branch);
    assert_eq!(published.sequence(), 1, "the fixture is snapshot 1");
    assert_eq!(model.len(), 64);
    assert_eq!(
        model.evaluate(&books(), &Projection::ID_ONLY).len(),
        BOOKS,
        "50 of the 64 walk records are books"
    );
    Fixture {
        engine,
        walk_ns,
        cost_ns,
        model,
    }
}

/// Publishes one disturbance per call and mirrors the published state in a
/// model of the head, so later disturbances pick rows that are still live
/// there. The pinned snapshot never sees any of it.
struct Head<'e> {
    engine: &'e mut Engine,
    ns: NamespaceId,
    model: Model,
    owner: OwnerId,
    revision: u64,
    step: usize,
    minted: u64,
    published: u64,
    stale_candidates: usize,
}

impl<'e> Head<'e> {
    fn new(engine: &'e mut Engine, ns: NamespaceId, model: Model) -> Self {
        Self {
            engine,
            ns,
            model,
            owner: ALICE,
            revision: 0,
            step: 0,
            minted: 0,
            published: 0,
            stale_candidates: 0,
        }
    }

    fn publish(&mut self, ops: Vec<WriteOp>) -> SnapshotId {
        let mut branch = begin(self.engine, None);
        for op in ops {
            apply(&mut branch, self.owner, op);
        }
        let id = publish(self.engine, branch);
        self.published += 1;
        id
    }

    /// A new book whose id lies beyond every pinned id.
    fn fresh_book(&mut self) -> (RecordKey, Vec<(CellName, Cell)>) {
        self.minted += 1;
        (
            key(format!("x{}", self.minted)),
            vec![
                (name("kind"), attr(Value::str("book"))),
                (name("price"), attr(Value::I64(1))),
                (name("stock"), attr(Value::U64(0))),
            ],
        )
    }

    /// The rows matching `query` at head beyond the paged prefix.
    fn unpaged(&self, query: &Query, paged: &[Row]) -> Vec<RecordKey> {
        let last = paged.last().map(|row| row.id);
        self.model
            .evaluate(query, &Projection::KEY_ONLY)
            .into_iter()
            .filter(|row| last.is_none_or(|last| row.id > last))
            .map(|row| row.key.expect("the key was projected"))
            .collect()
    }

    fn delete(&mut self, key: &RecordKey) {
        self.model.delete(key.as_bytes());
        self.publish(vec![delete_record(self.ns, key.as_bytes())]);
    }

    /// One disturbance, cycling through the seven kinds.
    fn disturb(&mut self, query: &Query, paged: &[Row]) {
        let step = self.step;
        self.step += 1;
        let unpaged = self.unpaged(query, paged);
        match step % 7 {
            // Delete an already-paged row: the latest one still live at head.
            0 => {
                let victim = paged
                    .iter()
                    .rev()
                    .map(|row| row.id)
                    .find_map(|id| self.model.record(id))
                    .map(|record| record.key.clone());
                match victim {
                    Some(key) => self.delete(&key),
                    None => {
                        self.publish(Vec::new());
                    }
                }
            }
            // Delete the next not-yet-paged match.
            1 => match unpaged.first() {
                Some(key) => self.delete(key),
                None => {
                    self.publish(Vec::new());
                }
            },
            // Patch the next not-yet-paged match so it stops matching: the
            // kind becomes pen, the price a field, the stock disappears.
            2 => match unpaged.first() {
                Some(key) => {
                    let set = vec![
                        (name("kind"), attr(Value::str("pen"))),
                        (name("price"), field(Value::I64(0))),
                    ];
                    let remove = vec![name("stock")];
                    self.model.patch(key.as_bytes(), &set, &remove);
                    self.publish(vec![patch_record(self.ns, key.as_bytes(), set, remove)]);
                }
                None => {
                    self.publish(Vec::new());
                }
            },
            // Create two matching records.
            3 => {
                let mut ops = Vec::new();
                for _ in 0..2 {
                    let (key, cells) = self.fresh_book();
                    self.model.create(key.as_bytes(), &cells);
                    ops.push(create_record(self.ns, key.as_bytes(), cells));
                }
                self.publish(ops);
            }
            // Transfer the namespace; later writes come from the new owner.
            4 => {
                let new_owner = if self.owner == ALICE { BOB } else { ALICE };
                self.publish(vec![transfer(self.ns, self.revision, new_owner)]);
                self.owner = new_owner;
                self.revision += 1;
            }
            // A competing candidate deletes the next not-yet-paged match but
            // loses publication to a candidate that creates a record.
            5 => {
                let head = self.engine.head();
                let mut winner = self.engine.begin(head, None).expect("at head");
                let mut loser = self.engine.begin(head, None).expect("at head");
                if let Some(key) = unpaged.first() {
                    let op = delete_record(self.ns, key.as_bytes());
                    apply(&mut loser, self.owner, op);
                }
                let (key, cells) = self.fresh_book();
                let expected = self.model.create(key.as_bytes(), &cells);
                let op = create_record(self.ns, key.as_bytes(), cells);
                let executed = apply(&mut winner, self.owner, op);
                assert_eq!(executed.effect, Effect::RecordCreated { id: expected });
                publish(self.engine, winner);
                self.published += 1;
                assert!(
                    matches!(
                        self.engine.publish(&loser.seal()),
                        Err(EngineError::StaleHead { .. })
                    ),
                    "the competing candidate is refused"
                );
                self.stale_candidates += 1;
            }
            // An empty block.
            _ => {
                self.publish(Vec::new());
            }
        }
    }

    fn finish(self) -> (Model, u64, usize) {
        (self.model, self.published, self.stale_candidates)
    }
}

#[test]
fn pinned_walks_match_the_reference_whether_or_not_the_head_advances() {
    let Fixture {
        mut engine,
        walk_ns,
        model,
        ..
    } = fixture();
    let pinned = engine.head_snapshot();
    let mut head = Head::new(&mut engine, walk_ns, model.clone());
    let projection = Projection::ALL;
    let mut expected_disturbances = 0;

    for (label, query) in shapes() {
        let reference = model.evaluate(&query, &projection);
        assert!(!reference.is_empty(), "{label}: the shape has matches");
        for limit in LIMITS {
            let quiet = walk(
                &pinned,
                walk_ns,
                &query,
                &projection,
                Budget::unlimited(),
                |_| limit,
                |_| {},
            );
            assert_eq!(rows_of(&quiet), reference, "{label}, limit {limit}");
            assert_eq!(
                quiet.len(),
                reference.len().div_ceil(limit),
                "{label}, limit {limit}: pages"
            );
            assert!(quiet.last().is_some_and(|page| page.next.is_none()));

            let disturbed = walk(
                &pinned,
                walk_ns,
                &query,
                &projection,
                Budget::unlimited(),
                |_| limit,
                |paged| head.disturb(&query, paged),
            );
            assert_eq!(
                disturbed, quiet,
                "{label}, limit {limit}: pages differ once the head advances"
            );
            expected_disturbances += quiet.len() - 1;
        }
    }
    assert_eq!(pinned.typed_record_count(walk_ns), Some(64));

    // Every disturbance published exactly one block (the losing candidates
    // none), and the head's state is what the head model says it is.
    let (at_head, published, stale_candidates) = head.finish();
    assert_eq!(
        published,
        u64::try_from(expected_disturbances).expect("fits")
    );
    assert!(
        stale_candidates >= 7 && published >= 100,
        "the head moved: {published} blocks, {stale_candidates} refused candidates"
    );
    let now = engine.head_snapshot();
    assert_eq!(now.id().sequence(), 1 + published);
    assert_eq!(now.typed_record_count(walk_ns), Some(at_head.len()));
    for (label, query) in shapes() {
        let head_rows = rows_of(&walk(
            &now,
            walk_ns,
            &query,
            &projection,
            Budget::unlimited(),
            |_| 7,
            |_| {},
        ));
        assert_eq!(
            head_rows,
            at_head.evaluate(&query, &projection),
            "{label} at head"
        );
    }
    let books_now = at_head.evaluate(&books(), &projection);
    assert_ne!(books_now, model.evaluate(&books(), &projection));
    assert!(
        books_now.iter().any(|row| row.id > rid(WALK_RECORDS)),
        "books created at head carry ids beyond the pinned ones"
    );
}

#[test]
fn a_walk_may_change_its_limit_between_pages() {
    let Fixture {
        engine,
        walk_ns,
        model,
        ..
    } = fixture();
    let pinned = engine.head_snapshot();
    let query = books();
    let projection = Projection::KEY_ONLY;
    let reference = model.evaluate(&query, &projection);

    // Limits 1, 3, 7, 2, 5 cycle: 18 rows per cycle, so the 50 books take
    // two full cycles (36), then 1 + 3 + 7 + 2 = 13 and a final page of 1
    // (limit 5) that is not full and has no continuation.
    let cycle = [1, 3, 7, 2, 5];
    let pages = walk(
        &pinned,
        walk_ns,
        &query,
        &projection,
        Budget::unlimited(),
        |page| cycle[page % cycle.len()],
        |_| {},
    );
    assert_eq!(rows_of(&pages), reference);
    let sizes: Vec<usize> = pages.iter().map(|page| page.rows.len()).collect();
    assert_eq!(sizes, [1, 3, 7, 2, 5, 1, 3, 7, 2, 5, 1, 3, 7, 2, 1]);
    assert!(pages[..14].iter().all(|page| page.next.is_some()));
    assert!(pages[14].next.is_none());

    // A `max_page` page after a small one returns everything left.
    let first = pinned
        .query(walk_ns, &query, &projection, 3, None, Budget::unlimited())
        .expect("the namespace exists");
    let cursor = first.value.next.expect("47 books remain");
    let rest = pinned
        .query(
            walk_ns,
            &query,
            &projection,
            MAX_PAGE,
            Some(&cursor),
            Budget::unlimited(),
        )
        .expect("the cursor is bound to this snapshot");
    assert_eq!(rest.value.rows, reference[3..]);
    assert_eq!(rest.value.next, None);
}

#[test]
fn eq_walk_charges_limit_plus_one_entries_then_exactly_the_remainder() {
    let Fixture {
        engine, cost_ns, ..
    } = fixture();
    let snapshot = engine.head_snapshot();
    let traced_budget = Budget::unlimited().traced();
    let query = books();
    let read = |limit: usize, cursor: Option<&Cursor>| {
        snapshot
            .query(
                cost_ns,
                &query,
                &Projection::ID_ONLY,
                limit,
                cursor,
                traced_budget,
            )
            .expect("the cost namespace exists")
    };
    let seek = [
        (OperationClass::DirectoryLookup, 1, 1, 1),
        (OperationClass::QueryGroup, 1, 5, 5),
        (OperationClass::TermSeek, 1, 4, 4),
    ];

    // (limit, cursor after, rows, next after, total, posting entries).
    // A page that leaves a further match reads `limit + 1` posting entries,
    // the last being the head beyond the page that proves the match exists;
    // the last page reads exactly the remainder, and an exactly full page is
    // a last page: `next` is `None` and no fourth entry is charged.
    type Page = (usize, Option<u64>, &'static [u64], Option<u64>, u64, u64);
    let pages: [Page; 7] = [
        (2, None, &[1, 2], Some(2), EQ_PAGE_ONE, 3),
        (2, Some(2), &[4], None, EQ_PAGE_TWO, 1),
        (3, None, &[1, 2, 4], None, EQ_FULL_PAGE, 3),
        (MAX_PAGE, None, &[1, 2, 4], None, EQ_FULL_PAGE, 3),
        (1, None, &[1], Some(1), EQ_LIMIT_ONE_PAGE, 2),
        (1, Some(1), &[2], Some(2), EQ_LIMIT_ONE_PAGE, 2),
        (1, Some(2), &[4], None, EQ_PAGE_TWO, 1),
    ];
    let mut cursors: BTreeMap<u64, Cursor> = BTreeMap::new();
    for (limit, after, rows, next, total, entries) in pages {
        let label = format!("limit {limit} after {after:?}");
        let page = read(limit, after.map(|after| &cursors[&after]));
        let expected: Vec<Row> = rows.iter().map(|&ordinal| id_row(ordinal)).collect();
        assert_eq!(page.value.rows, expected, "{label}");
        let emitted = rows.len() as u64;
        assert_eq!(
            page.receipt,
            traced(
                read_applied(total),
                ledger(&[
                    seek[0],
                    seek[1],
                    seek[2],
                    (OperationClass::PostingEntry, entries, 1, entries),
                    (OperationClass::ResultRow, emitted, 3, 3 * emitted),
                ])
            ),
            "{label}"
        );
        assert_eq!(
            page.value.next.as_ref().map(|cursor| cursor.after()),
            next.map(rid),
            "{label}"
        );
        if let Some(cursor) = page.value.next {
            assert_eq!(cursor.snapshot(), Some(snapshot.id()), "{label}");
            cursors.insert(cursor.after().ordinal(), cursor);
        }
    }

    // A value nobody holds costs the seek only and claims no continuation.
    let none = snapshot
        .query(
            cost_ns,
            &kind_is("desk"),
            &Projection::ID_ONLY,
            2,
            None,
            traced_budget,
        )
        .expect("the cost namespace exists");
    assert_eq!(none.value.rows, []);
    assert_eq!(none.value.next, None);
    assert_eq!(
        none.receipt,
        traced(read_applied(EQ_MISSING_TERM), ledger(&seek))
    );

    // With `Projection::ALL` every emitted row adds its 22 projected bytes.
    let all = snapshot
        .query(cost_ns, &query, &Projection::ALL, 2, None, traced_budget)
        .expect("the cost namespace exists");
    assert_eq!(
        all.value.rows,
        [cost_row(1, "a", "book", 10), cost_row(2, "b", "book", 20)]
    );
    assert_eq!(all.value.next.map(|cursor| cursor.after()), Some(rid(2)));
    assert_eq!(
        all.receipt,
        traced(
            read_applied(EQ_PAGE_ONE_ALL),
            ledger(&[
                seek[0],
                seek[1],
                seek[2],
                (OperationClass::PostingEntry, 3, 1, 3),
                (OperationClass::ResultRow, 2, 3, 6),
                (OperationClass::ReadByte, 44, 1, 44),
            ])
        )
    );
}

#[test]
fn a_group_whose_head_is_beyond_the_page_end_does_no_further_work() {
    let Fixture {
        engine, cost_ns, ..
    } = fixture();
    let snapshot = engine.head_snapshot();
    let traced_budget = Budget::unlimited().traced();
    let query = books_or_priced_lamps();
    let read = |cursor: Option<&Cursor>| {
        snapshot
            .query(
                cost_ns,
                &query,
                &Projection::ID_ONLY,
                2,
                cursor,
                traced_budget,
            )
            .expect("the cost namespace exists")
    };
    let open = [
        (OperationClass::DirectoryLookup, 1, 1, 1),
        (OperationClass::QueryGroup, 2, 5, 10),
        (OperationClass::TermSeek, 2, 4, 8),
    ];

    // Page 1: the lamp group pulls its head (rec:5, one entry and one probe)
    // and is then left alone, although its posting holds rec:6 too.
    let first = read(None);
    assert_eq!(first.value.rows, [id_row(1), id_row(2)]);
    let cursor = first.value.next.clone().expect("rec:4 follows");
    assert_eq!(cursor.after(), rid(2));
    assert_eq!(
        first.receipt,
        traced(
            read_applied(OR_PAGE_ONE),
            ledger(&[
                open[0],
                open[1],
                open[2],
                (OperationClass::PostingEntry, 4, 1, 4),
                (OperationClass::RecordProbe, 1, 2, 2),
                (OperationClass::ResultRow, 2, 3, 6),
            ])
        )
    );

    // Page 2: rec:4 exhausts the book group for free; emitting rec:5 is what
    // advances the lamp group to rec:6, the head beyond this page.
    let second = read(Some(&cursor));
    assert_eq!(second.value.rows, [id_row(4), id_row(5)]);
    let cursor = second.value.next.clone().expect("rec:6 follows");
    assert_eq!(cursor.after(), rid(5));
    assert_eq!(
        second.receipt,
        traced(
            read_applied(OR_PAGE_TWO),
            ledger(&[
                open[0],
                open[1],
                open[2],
                (OperationClass::PostingEntry, 3, 1, 3),
                (OperationClass::RecordProbe, 2, 2, 4),
                (OperationClass::ResultRow, 2, 3, 6),
            ])
        )
    );

    // Page 3: the book posting has nothing past rec:5 (no entry charged);
    // the lamp group yields rec:6 and the walk ends.
    let third = read(Some(&cursor));
    assert_eq!(third.value.rows, [id_row(6)]);
    assert_eq!(third.value.next, None);
    assert_eq!(
        third.receipt,
        traced(
            read_applied(OR_PAGE_THREE),
            ledger(&[
                open[0],
                open[1],
                open[2],
                (OperationClass::PostingEntry, 1, 1, 1),
                (OperationClass::RecordProbe, 1, 2, 2),
                (OperationClass::ResultRow, 1, 3, 3),
            ])
        )
    );
}

#[test]
fn cursor_from_another_snapshot_or_a_branch_is_a_mismatch() {
    let Fixture {
        mut engine,
        walk_ns,
        ..
    } = fixture();
    let query = books();
    let projection = Projection::ALL;
    let one = engine.head_snapshot();
    let page_one = one
        .query(walk_ns, &query, &projection, 7, None, Budget::unlimited())
        .expect("the walk namespace exists");
    let cursor_one = page_one.value.next.clone().expect("43 books remain");

    // An empty block: snapshot 2 has the identical (physically shared)
    // contents, yet it is another snapshot.
    let empty = begin(&engine, None);
    let two_id = publish(&mut engine, empty);
    let two = engine.snapshot(two_id).expect("just published");
    assert!(two.typed_shared_with(&one, walk_ns));
    let page_two = two
        .query(walk_ns, &query, &projection, 7, None, Budget::unlimited())
        .expect("the walk namespace exists");
    assert_eq!(page_two.value.rows, page_one.value.rows);
    assert_eq!(page_two.receipt, page_one.receipt);
    let cursor_two = page_two.value.next.clone().expect("43 books remain");
    assert_ne!(cursor_two, cursor_one);
    assert_eq!(cursor_one.snapshot(), Some(one.id()));
    assert_eq!(cursor_two.snapshot(), Some(two_id));
    assert_eq!(cursor_two.after(), cursor_one.after());

    assert_mismatch(
        two.query(
            walk_ns,
            &query,
            &projection,
            7,
            Some(&cursor_one),
            Budget::unlimited(),
        ),
        "snapshot 1's cursor at snapshot 2",
    );
    assert_mismatch(
        one.query(
            walk_ns,
            &query,
            &projection,
            7,
            Some(&cursor_two),
            Budget::unlimited(),
        ),
        "snapshot 2's cursor at snapshot 1",
    );
    // A traced mismatch carries an empty ledger: nothing was charged.
    let traced_mismatch = expect_rejected(
        two.query(
            walk_ns,
            &query,
            &projection,
            7,
            Some(&cursor_one),
            Budget::unlimited().traced(),
        ),
        "traced mismatch",
    );
    assert_eq!(
        traced_mismatch.receipt,
        traced(read_receipt(0, CURSOR_MISMATCH), ledger(&[]))
    );
    // Each cursor continues at home.
    let home_one = one
        .query(
            walk_ns,
            &query,
            &projection,
            7,
            Some(&cursor_one),
            Budget::unlimited(),
        )
        .expect("bound to snapshot 1");
    let home_two = two
        .query(
            walk_ns,
            &query,
            &projection,
            7,
            Some(&cursor_two),
            Budget::unlimited(),
        )
        .expect("bound to snapshot 2");
    assert_eq!(home_one.value.rows, home_two.value.rows);
    // Another handle to snapshot 1 is the same snapshot: the cursor is
    // accepted there and the page is the same.
    let one_again = engine
        .snapshot(one.id())
        .expect("published snapshots resolve");
    let through_again = one_again
        .query(
            walk_ns,
            &query,
            &projection,
            7,
            Some(&cursor_one),
            Budget::unlimited(),
        )
        .expect("bound to snapshot 1");
    assert_eq!(through_again.value, home_one.value);
    assert_eq!(through_again.receipt, home_one.receipt);

    // A branch refuses a snapshot cursor, its base refuses the branch's
    // cursor, and so does the snapshot the branch publishes.
    let branch = begin(&engine, None);
    assert_mismatch(
        branch.query(
            walk_ns,
            &query,
            &projection,
            7,
            Some(&cursor_two),
            Budget::unlimited(),
        ),
        "the base snapshot's cursor on a branch",
    );
    let branch_page = branch
        .query(walk_ns, &query, &projection, 7, None, Budget::unlimited())
        .expect("the walk namespace exists");
    assert_eq!(branch_page.value.rows, page_two.value.rows);
    let branch_cursor = branch_page.value.next.expect("43 books remain");
    assert_eq!(branch_cursor.snapshot(), None);
    assert_mismatch(
        two.query(
            walk_ns,
            &query,
            &projection,
            7,
            Some(&branch_cursor),
            Budget::unlimited(),
        ),
        "a branch cursor on the branch's base",
    );
    let three_id = publish(&mut engine, branch);
    let three = engine.snapshot(three_id).expect("just published");
    assert_mismatch(
        three.query(
            walk_ns,
            &query,
            &projection,
            7,
            Some(&branch_cursor),
            Budget::unlimited(),
        ),
        "a branch cursor on the snapshot the branch published",
    );
}

#[test]
fn cursor_from_another_engine_at_the_same_snapshot_id_is_a_mismatch() {
    let left = fixture();
    let right = fixture();
    let query = books();
    let projection = Projection::ALL;
    let left_snapshot = left.engine.head_snapshot();
    let right_snapshot = right.engine.head_snapshot();
    assert_eq!(left_snapshot.id(), right_snapshot.id());
    assert_eq!(left.walk_ns, right.walk_ns);
    let ns = left.walk_ns;

    let left_page = left_snapshot
        .query(ns, &query, &projection, 7, None, Budget::unlimited())
        .expect("the walk namespace exists");
    let right_page = right_snapshot
        .query(ns, &query, &projection, 7, None, Budget::unlimited())
        .expect("the walk namespace exists");
    assert_eq!(left_page.value.rows, right_page.value.rows);
    assert_eq!(left_page.receipt, right_page.receipt);
    let left_cursor = left_page.value.next.expect("43 books remain");
    let right_cursor = right_page.value.next.expect("43 books remain");
    assert_eq!(left_cursor.after(), right_cursor.after());
    assert_eq!(left_cursor.snapshot(), right_cursor.snapshot());

    assert_mismatch(
        right_snapshot.query(
            ns,
            &query,
            &projection,
            7,
            Some(&left_cursor),
            Budget::unlimited(),
        ),
        "the left engine's cursor at the right engine",
    );
    assert_mismatch(
        left_snapshot.query(
            ns,
            &query,
            &projection,
            7,
            Some(&right_cursor),
            Budget::unlimited(),
        ),
        "the right engine's cursor at the left engine",
    );
    let left_second = left_snapshot
        .query(
            ns,
            &query,
            &projection,
            7,
            Some(&left_cursor),
            Budget::unlimited(),
        )
        .expect("bound to the left engine");
    let right_second = right_snapshot
        .query(
            ns,
            &query,
            &projection,
            7,
            Some(&right_cursor),
            Budget::unlimited(),
        )
        .expect("bound to the right engine");
    assert_eq!(left_second.value.rows, right_second.value.rows);
}

#[test]
fn cursor_bound_to_another_namespace_query_or_projection_is_a_mismatch() {
    let Fixture {
        engine,
        walk_ns,
        cost_ns,
        model,
    } = fixture();
    let snapshot = engine.head_snapshot();
    let query = books();
    let projection = Projection::ALL;
    let reference = model.evaluate(&query, &projection);
    let page = snapshot
        .query(walk_ns, &query, &projection, 7, None, Budget::unlimited())
        .expect("the walk namespace exists");
    let cursor = page.value.next.expect("43 books remain");
    // The first seven books are records r00..r07 minus the pen r04: ids
    // 1, 2, 3, 4, 6, 7, 8.
    assert_eq!(cursor.after(), rid(8));
    assert_eq!(cursor.after(), reference[6].id);
    assert_eq!(cursor.namespace(), walk_ns);
    assert_eq!(cursor.query(), &query);
    assert_eq!(cursor.projection(), &projection);

    // Another namespace, although it holds books too.
    assert_mismatch(
        snapshot.query(
            cost_ns,
            &query,
            &projection,
            7,
            Some(&cursor),
            Budget::unlimited(),
        ),
        "another namespace",
    );

    // A different query: another value, the same literal in two groups, a
    // second literal.
    for (label, other) in [
        ("another value", kind_is("lamp")),
        (
            "the same literal in two groups",
            Query {
                groups: vec![query.groups[0].clone(), query.groups[0].clone()],
            },
        ),
        (
            "a second literal",
            Query::all(vec![
                Literal::of(Term::eq(name("kind"), Value::str("book"))),
                Literal::of(Term::exists(name("price"), TypeTag::I64)),
            ]),
        ),
    ] {
        assert_mismatch(
            snapshot.query(
                walk_ns,
                &other,
                &projection,
                7,
                Some(&cursor),
                Budget::unlimited(),
            ),
            label,
        );
    }
    // The same literals in another order are another query.
    let ordered_one = Query::all(vec![
        Literal::of(Term::exists(name("price"), TypeTag::I64)),
        Literal::of(Term::eq(name("kind"), Value::str("book"))),
    ]);
    let ordered_two = Query::all(vec![
        Literal::of(Term::eq(name("kind"), Value::str("book"))),
        Literal::of(Term::exists(name("price"), TypeTag::I64)),
    ]);
    let ordered_cursor = snapshot
        .query(
            walk_ns,
            &ordered_one,
            &projection,
            7,
            None,
            Budget::unlimited(),
        )
        .expect("the walk namespace exists")
        .value
        .next
        .expect("further matches exist");
    assert_mismatch(
        snapshot.query(
            walk_ns,
            &ordered_two,
            &projection,
            7,
            Some(&ordered_cursor),
            Budget::unlimited(),
        ),
        "the same literals in another order",
    );

    // A different projection.
    for (label, other) in [
        ("ids only", Projection::ID_ONLY),
        ("the key only", Projection::KEY_ONLY),
        ("named cells", Projection::named(true, [name("kind")])),
    ] {
        assert_mismatch(
            snapshot.query(
                walk_ns,
                &query,
                &other,
                7,
                Some(&cursor),
                Budget::unlimited(),
            ),
            label,
        );
    }
    let named = Projection::named(true, [name("kind")]);
    let named_cursor = snapshot
        .query(walk_ns, &query, &named, 7, None, Budget::unlimited())
        .expect("the walk namespace exists")
        .value
        .next
        .expect("43 books remain");
    assert_mismatch(
        snapshot.query(
            walk_ns,
            &query,
            &Projection::named(false, [name("kind")]),
            7,
            Some(&named_cursor),
            Budget::unlimited(),
        ),
        "the same names without the key",
    );
}

#[test]
fn cursor_check_comes_after_query_and_projection_validation() {
    let Fixture {
        mut engine,
        walk_ns,
        ..
    } = fixture();
    let query = books();
    let projection = Projection::ALL;
    let one = engine.head_snapshot();
    let foreign = one
        .query(walk_ns, &query, &projection, 7, None, Budget::unlimited())
        .expect("the walk namespace exists")
        .value
        .next
        .expect("43 books remain");
    let empty = begin(&engine, None);
    let two_id = publish(&mut engine, empty);
    let two = engine.snapshot(two_id).expect("just published");

    // The query shape, the limit and the projection are checked first.
    assert_zero_cost(
        two.query(
            walk_ns,
            &Query { groups: Vec::new() },
            &projection,
            7,
            Some(&foreign),
            Budget::unlimited(),
        ),
        Rejection::InvalidQuery(QueryFault::NoGroups),
        "no groups",
    );
    assert_zero_cost(
        two.query(
            walk_ns,
            &query,
            &projection,
            0,
            Some(&foreign),
            Budget::unlimited(),
        ),
        Rejection::InvalidQuery(QueryFault::ZeroLimit),
        "zero limit",
    );
    assert_zero_cost(
        two.query(
            walk_ns,
            &query,
            &projection,
            MAX_PAGE + 1,
            Some(&foreign),
            Budget::unlimited(),
        ),
        Rejection::InvalidQuery(QueryFault::LimitTooLarge),
        "limit too large",
    );
    let too_many = Projection::named(true, (0..65).map(|n| name(&format!("c{n}"))));
    assert_zero_cost(
        two.query(
            walk_ns,
            &query,
            &too_many,
            7,
            Some(&foreign),
            Budget::unlimited(),
        ),
        Rejection::InvalidQuery(QueryFault::ProjectionTooLarge),
        "projection too large",
    );
    // Only a valid request reaches the cursor check.
    assert_mismatch(
        two.query(
            walk_ns,
            &query,
            &projection,
            7,
            Some(&foreign),
            Budget::unlimited(),
        ),
        "a valid request with a foreign cursor",
    );
    // An unknown namespace is charged (the directory lookup) but a foreign
    // cursor is refused first, at admission.
    let unknown = ns(99);
    assert_mismatch(
        two.query(
            unknown,
            &query,
            &projection,
            7,
            Some(&foreign),
            Budget::unlimited(),
        ),
        "a foreign cursor on an unknown namespace",
    );
}

/// A fresh branch on the fixture head with the first page (limit 2) of the
/// cost namespace's books read: rows `[1, 2]`, cursor after 2.
fn branch_with_cursor(engine: &Engine, cost_ns: NamespaceId) -> (Branch, Cursor) {
    let branch = begin(engine, None);
    let page = branch
        .query(
            cost_ns,
            &books(),
            &Projection::ID_ONLY,
            2,
            None,
            Budget::unlimited(),
        )
        .expect("the cost namespace exists");
    assert_eq!(page.value.rows, [id_row(1), id_row(2)]);
    assert_eq!(page.receipt, read_applied(EQ_PAGE_ONE));
    let cursor = page.value.next.expect("rec:4 follows");
    assert_eq!(cursor.after(), rid(2));
    assert_eq!(cursor.snapshot(), None, "a branch cursor names no snapshot");
    (branch, cursor)
}

fn second_page(branch: &Branch, cost_ns: NamespaceId, cursor: &Cursor) -> ReadResult<QueryPage> {
    branch.query(
        cost_ns,
        &books(),
        &Projection::ID_ONLY,
        2,
        Some(cursor),
        Budget::unlimited().traced(),
    )
}

fn create_g(cost_ns: NamespaceId) -> WriteOp {
    create_record(cost_ns, "g", cost_cells("book", 40))
}

#[test]
fn branch_cursor_continues_on_the_same_untouched_branch() {
    let Fixture {
        engine, cost_ns, ..
    } = fixture();
    let mut branch = begin(&engine, None);
    // A branch-local fourth book, id 7, visible to the branch's own pages.
    let created = apply(&mut branch, ALICE, create_g(cost_ns));
    assert_eq!(created.effect, Effect::RecordCreated { id: rid(7) });
    let first = branch
        .query(
            cost_ns,
            &books(),
            &Projection::ID_ONLY,
            2,
            None,
            Budget::unlimited().traced(),
        )
        .expect("the cost namespace exists");
    assert_eq!(first.value.rows, [id_row(1), id_row(2)]);
    assert_eq!(first.receipt.spent, Cost::new(EQ_PAGE_ONE));
    let cursor = first.value.next.expect("rec:4 and rec:7 follow");
    assert_eq!(cursor.snapshot(), None);

    // Nothing below advances the branch's mutation serial: estimates,
    // a checkpoint, writes rejected after charging and writes rejected at
    // admission.
    let estimated = branch
        .estimate(
            ALICE,
            create_record(cost_ns, "h", cost_cells("book", 50)),
            Budget::unlimited(),
        )
        .expect("the estimate applies on scratch state");
    assert_eq!(estimated.effect, Effect::RecordCreated { id: rid(8) });
    branch
        .estimate_host(delete_record(cost_ns, "c"), Budget::unlimited())
        .expect("the host estimate applies on scratch state");
    let _checkpoint = branch.checkpoint();
    let unauthorized = branch
        .execute(
            BOB,
            create_record(cost_ns, "h", cost_cells("book", 50)),
            Budget::unlimited(),
        )
        .expect_err("BOB does not own the namespace");
    assert_eq!(unauthorized.reason, Rejection::Unauthorized);
    assert_eq!(unauthorized.receipt.spent, Cost::new(1));
    let not_found = branch
        .execute(ALICE, delete_record(cost_ns, "zz"), Budget::unlimited())
        .expect_err("zz is not bound");
    assert_eq!(not_found.reason, Rejection::NotFound);
    assert_eq!(not_found.receipt.spent, Cost::new(4));
    let empty_patch = branch
        .execute(
            ALICE,
            patch_record(cost_ns, "zz", Vec::new(), Vec::new()),
            Budget::unlimited(),
        )
        .expect_err("an empty patch of an unbound key is not found");
    assert_eq!(empty_patch.reason, Rejection::NotFound);
    assert_eq!(empty_patch.receipt.spent, Cost::new(4));
    let not_host = branch
        .execute_host(create_namespace("test"), Budget::unlimited())
        .expect_err("the host path accepts record operations only");
    assert_eq!(
        not_host.reason,
        Rejection::InvalidArgument(arkiv_db::ArgumentFault::NotHostOperation)
    );
    assert_eq!(not_host.receipt.spent, Cost::new(0));
    assert_eq!(
        branch.receipts().len(),
        4,
        "the create and the three charged rejections were recorded"
    );

    let second = second_page(&branch, cost_ns, &cursor).expect("the cursor is still bound");
    assert_eq!(second.value.rows, [id_row(4), id_row(7)]);
    assert_eq!(second.value.next, None);
    assert_eq!(
        second.receipt,
        traced(
            read_applied(EQ_BRANCH_PAGE_TWO),
            ledger(&[
                (OperationClass::DirectoryLookup, 1, 1, 1),
                (OperationClass::QueryGroup, 1, 5, 5),
                (OperationClass::TermSeek, 1, 4, 4),
                (OperationClass::PostingEntry, 2, 1, 2),
                (OperationClass::ResultRow, 2, 3, 6),
            ])
        )
    );
    // Replaying the branch page is identical too.
    let replay = second_page(&branch, cost_ns, &cursor).expect("still bound");
    assert_eq!(replay.value, second.value);
    assert_eq!(replay.receipt, second.receipt);
}

#[test]
fn branch_cursor_is_invalidated_by_every_applied_mutation() {
    let Fixture {
        engine, cost_ns, ..
    } = fixture();
    let renewed_page_two = |branch: &Branch| {
        let first = branch
            .query(
                cost_ns,
                &books(),
                &Projection::ID_ONLY,
                2,
                None,
                Budget::unlimited(),
            )
            .expect("the cost namespace exists");
        assert_eq!(first.value.rows, [id_row(1), id_row(2)]);
        let cursor = first.value.next.expect("rec:4 follows");
        second_page(branch, cost_ns, &cursor).expect("a cursor issued at the current serial")
    };

    // An applied execute.
    let (mut branch, cursor) = branch_with_cursor(&engine, cost_ns);
    apply(&mut branch, ALICE, create_g(cost_ns));
    assert_mismatch(
        second_page(&branch, cost_ns, &cursor),
        "after an applied execute",
    );
    let renewed = renewed_page_two(&branch);
    assert_eq!(renewed.value.rows, [id_row(4), id_row(7)]);
    assert_eq!(renewed.receipt.spent, Cost::new(EQ_BRANCH_PAGE_TWO));

    // An applied execute_host, even one that leaves the result set as it
    // was (it deletes the pen).
    let (mut branch, cursor) = branch_with_cursor(&engine, cost_ns);
    branch
        .execute_host(delete_record(cost_ns, "c"), Budget::unlimited())
        .expect("the host deletes without an owner check");
    assert_mismatch(
        second_page(&branch, cost_ns, &cursor),
        "after an execute_host",
    );
    let renewed = renewed_page_two(&branch);
    assert_eq!(renewed.value.rows, [id_row(4)]);
    assert_eq!(renewed.receipt.spent, Cost::new(EQ_PAGE_TWO));

    // A rollback, even one that discards nothing.
    let (mut branch, cursor) = branch_with_cursor(&engine, cost_ns);
    let checkpoint = branch.checkpoint();
    assert_eq!(branch.rollback_to(checkpoint), Ok(0..0));
    assert_mismatch(second_page(&branch, cost_ns, &cursor), "after a rollback");
    assert_eq!(renewed_page_two(&branch).value.rows, [id_row(4)]);

    // A pricing change: the cursor is gone, yet the branch's reads stay
    // priced with the pinned schedule.
    let (mut branch, cursor) = branch_with_cursor(&engine, cost_ns);
    branch
        .set_pricing(PriceSchedule::FIXTURE.with_id(2))
        .expect("id 2 exceeds the committed id 1");
    assert_mismatch(second_page(&branch, cost_ns, &cursor), "after set_pricing");
    let renewed = renewed_page_two(&branch);
    assert_eq!(renewed.value.rows, [id_row(4)]);
    assert_eq!(
        renewed.receipt.schedule_id, FIXTURE_ID,
        "the branch prices reads with its base's schedule"
    );
    assert_eq!(renewed.receipt.spent, Cost::new(EQ_PAGE_TWO));

    // Another branch on the same base, untouched, is another branch.
    let (branch_a, cursor_a) = branch_with_cursor(&engine, cost_ns);
    let (branch_b, cursor_b) = branch_with_cursor(&engine, cost_ns);
    assert_ne!(cursor_a, cursor_b);
    assert_mismatch(second_page(&branch_b, cost_ns, &cursor_a), "another branch");
    assert_mismatch(second_page(&branch_a, cost_ns, &cursor_b), "another branch");
    assert_eq!(
        second_page(&branch_a, cost_ns, &cursor_a)
            .expect("at home")
            .value
            .rows,
        [id_row(4)]
    );
}

#[test]
fn host_resolves_an_omitted_block_once_and_pages_that_snapshot() {
    let Fixture {
        mut engine,
        walk_ns,
        model,
        ..
    } = fixture();
    let query = books();
    let projection = Projection::ALL;
    let reference = model.evaluate(&query, &projection);

    // A request that omits the block: the host resolves the head once and
    // pins that snapshot for the whole walk.
    let pinned = engine.head_snapshot();
    assert_eq!(pinned.id().sequence(), 1);

    // The total is a separate metered read at the same snapshot.
    let total = pinned
        .count(walk_ns, &query, Budget::unlimited())
        .expect("the walk namespace exists");
    assert_eq!(total.value, BOOKS as u64);
    assert_eq!(total.receipt, read_applied(COUNT_BOOKS));

    // Between pages the chain produces a block: it deletes the next book at
    // head (the very row the pinned walk returns next) and the first one
    // also commits a new pricing schedule.
    let mut at_head = model;
    let mut deleted: Vec<RecordId> = Vec::new();
    let pages = walk(
        &pinned,
        walk_ns,
        &query,
        &projection,
        Budget::unlimited(),
        |_| 7,
        |paged| {
            let last = paged.last().expect("a continued page has rows").id;
            let victim = at_head
                .evaluate(&query, &Projection::KEY_ONLY)
                .into_iter()
                .find(|row| row.id > last)
                .expect("the head still holds books beyond the paged prefix");
            let key = victim.key.expect("the key was projected");
            let mut branch = begin(&engine, None);
            apply(&mut branch, ALICE, delete_record(walk_ns, key.as_bytes()));
            if deleted.is_empty() {
                branch
                    .set_pricing(PriceSchedule::FIXTURE.with_id(2))
                    .expect("id 2 exceeds the committed id 1");
            }
            at_head.delete(key.as_bytes());
            deleted.push(victim.id);
            publish(&mut engine, branch);
        },
    );
    assert_eq!(rows_of(&pages), reference);
    assert_eq!(pages.len(), 8, "ceil(50 / 7)");
    assert_eq!(deleted.len(), 7);
    assert!(
        pages
            .iter()
            .all(|page| page.receipt.schedule_id == FIXTURE_ID),
        "the pinned snapshot prices every page with its own schedule"
    );
    // Every row deleted at head was still returned by the pinned walk, on
    // the page right after its deletion.
    for (index, id) in deleted.iter().enumerate() {
        assert_eq!(pages[index + 1].rows[0].id, *id);
    }
    assert_eq!(
        pinned
            .count(walk_ns, &query, Budget::unlimited())
            .expect("the walk namespace exists")
            .value,
        BOOKS as u64
    );

    // The head moved on: a fresh resolution prices reads with schedule 2,
    // answers with 43 books and refuses the old cursor.
    let now = engine.head_snapshot();
    assert_eq!(now.id().sequence(), 8);
    assert_eq!(now.pricing().id(), 2);
    let now_total = now
        .count(walk_ns, &query, Budget::unlimited())
        .expect("the walk namespace exists");
    assert_eq!(now_total.value, 43);
    assert_eq!(
        now_total.receipt,
        receipt(
            ReceiptKind::Read,
            2,
            COUNT_BOOKS_AFTER_DELETES,
            Outcome::Applied
        )
    );
    let now_rows = rows_of(&walk(
        &now,
        walk_ns,
        &query,
        &projection,
        Budget::unlimited(),
        |_| 7,
        |_| {},
    ));
    assert_eq!(now_rows, at_head.evaluate(&query, &projection));
    assert_eq!(now_rows.len(), 43);
    assert!(
        deleted
            .iter()
            .all(|id| !now_rows.iter().any(|row| row.id == *id))
    );
    // The refusal is priced (at zero) under the new snapshot's own schedule.
    let first_cursor = pages[0].next.clone().expect("page 1 continued");
    assert_zero_cost_under(
        now.query(
            walk_ns,
            &query,
            &projection,
            7,
            Some(&first_cursor),
            Budget::unlimited(),
        ),
        Rejection::CursorMismatch,
        2,
        "the pinned walk's cursor at the new latest snapshot",
    );
    // The pinned handle still answers.
    let again = pinned
        .query(
            walk_ns,
            &query,
            &projection,
            7,
            Some(&first_cursor),
            Budget::unlimited(),
        )
        .expect("bound to the pinned snapshot");
    assert_eq!(again.value.rows, pages[1].rows);
}

#[test]
fn a_cursor_after_any_record_continues_like_an_issued_one() {
    let Fixture {
        engine,
        walk_ns,
        model,
        ..
    } = fixture();
    let pinned = engine.head_snapshot();
    let query = books();
    let ids = model.evaluate(&query, &Projection::ID_ONLY);
    let reference = model.evaluate(&query, &Projection::KEY_ONLY);

    // Skip ten matches by id, then read on with the projection the page
    // returns: the same rows as an issued KEY_ONLY cursor after them.
    let skipped = pinned
        .query(
            walk_ns,
            &query,
            &Projection::ID_ONLY,
            10,
            None,
            Budget::unlimited(),
        )
        .expect("the namespace exists")
        .value;
    assert_eq!(skipped.rows, ids[..10]);
    let after = skipped.rows[9].id;
    let made = pinned.cursor_after(walk_ns, &query, &Projection::KEY_ONLY, after);
    assert_eq!(made.after(), after);
    assert_eq!(made.snapshot(), Some(pinned.id()));
    let issued = pinned
        .query(
            walk_ns,
            &query,
            &Projection::KEY_ONLY,
            10,
            None,
            Budget::unlimited(),
        )
        .expect("the namespace exists")
        .value
        .next
        .expect("40 books remain");
    assert_eq!(made, issued);
    let rest = pinned
        .query(
            walk_ns,
            &query,
            &Projection::KEY_ONLY,
            MAX_PAGE,
            Some(&made),
            Budget::unlimited(),
        )
        .expect("a made cursor is bound to its snapshot");
    assert_eq!(rest.value.rows, reference[10..]);

    // Any id is a position, a non-matching or absent one included: the page
    // starts at the first match above it.
    let gap = (10..ids.len() - 1)
        .find(|&k| ids[k + 1].id.ordinal() > ids[k].id.ordinal() + 1)
        .expect("pens and lamps sit between books");
    let between = rid(ids[gap].id.ordinal() + 1);
    assert!(!ids.iter().any(|row| row.id == between));
    let from_gap = pinned
        .query(
            walk_ns,
            &query,
            &Projection::ID_ONLY,
            1,
            Some(&pinned.cursor_after(walk_ns, &query, &Projection::ID_ONLY, between)),
            Budget::unlimited(),
        )
        .expect("bound");
    assert_eq!(from_gap.value.rows, ids[gap + 1..gap + 2]);
    let past = pinned
        .query(
            walk_ns,
            &query,
            &Projection::ID_ONLY,
            1,
            Some(&pinned.cursor_after(walk_ns, &query, &Projection::ID_ONLY, rid(1_000))),
            Budget::unlimited(),
        )
        .expect("bound");
    assert_eq!((past.value.rows.len(), past.value.next), (0, None));

    // It is bound like an issued cursor: another projection is a mismatch.
    assert_mismatch(
        pinned.query(
            walk_ns,
            &query,
            &Projection::ALL,
            1,
            Some(&made),
            Budget::unlimited(),
        ),
        "a made cursor under another projection",
    );
}
