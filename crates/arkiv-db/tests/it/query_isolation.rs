//! Cross-namespace isolation of the typed record store, its term indexes and
//! every query operator, in the shape of `isolation.rs`.
//!
//! Namespace A holds 40 typed records over five cell names (`kind: Str`,
//! `price: I64`, `hot: Bool` and `$exp: U64` attributes plus a `note: Bytes`
//! field) and two raw byte records. Namespace B holds five typed records
//! carrying exactly the same names and values (plus `seq: I64` and
//! `label: Str`). A's workload runs on a branch from the current head that is
//! dropped, never sealed or published, so A's published state is frozen: it
//! creates records whose terms exist and whose terms are new, patches (value
//! change, type change), deletes, host-path deletes, an expiry sweep,
//! `get_record` hits and misses, and queries with `Eq`, `NotEq`, `Range`,
//! `StartsWith`, `Exists`, a negated literal, match-all, an AND-group with
//! probes, an OR of three groups, a three-page cursor walk, a `Named`
//! projection, counts, and a budget-limited variant of each that aborts
//! mid-enumeration. Phase 1 publishes 10 000 typed records into B carrying
//! A's attribute names and values plus thousands of distinct `seq` and
//! `label` terms, and 2 000 raw records; phase 2 publishes 500 namespaces.
//!
//! Every receipt (ledgers included), result, abort point and effect of A's
//! workload is a hand-derived literal, and every result also agrees with a
//! full-scan evaluation over a `BTreeMap` model of A's records. All of it,
//! and A's namespace-local physical counters, must be identical in every
//! phase and on a snapshot pinned after phase 0; the directory work must be
//! exactly A's lookup path in the directory trie. State roots are covered by
//! `roots.rs`.

use std::{collections::HashSet, ops::Bound};

use arkiv_db::{
    ArgumentFault, BlockNumber, Branch, Budget, Cell, CellName, Cost, Cursor, Effect, Engine,
    ExpiryHalt, Limits, Literal, NamespaceId,
    OperationClass::{
        CellByte, CellRemove, CellWrite, DirectoryLookup, IndexInsert, IndexRemove,
        IndexTermCreate, PointRead, PostingEntry, QueryGroup, ReadByte, RecordCreate, RecordDelete,
        RecordPatch, RecordProbe, ResultRow, RootUpdate, RowByte, RowVisit, ScanEntry, ScanSeek,
        TermSeek, TermVisit,
    },
    PhysicalTrace, PriceSchedule, Projection, Query, QueryPage, ReadResult, Receipt, RecordId,
    Rejection, Row, Snapshot, SnapshotId, Term, TypeTag, Value, expire_records,
};

use crate::support::{
    isolation::{
        DIRECTORY_PATH_BOUND, Expected, Step, assert_directory_nodes_equal,
        assert_directory_nodes_follow_the_trie, assert_matches_expected,
        assert_namespace_local_nodes_equal, assert_receipts_and_results_equal, assert_steps_repeat,
        expected_receipt, find, itemized, publish_block, read_step, write_step,
    },
    model::{Model, cell_bytes, project, query_holds},
    reader::Reader,
    vocab::{
        ALICE, BOB, DAVE, add_namespace, applied, apply, attr, begin, create_record, delete_record,
        descriptor, field, key, name, patch_record, put, read_applied, read_rejected, rejected,
        rid,
    },
};

/// A's typed records `a/00`..`a/39` (4-byte keys) get ids 1..=40 in creation
/// order; see [`a_cells`] for their cells.
const A_RECORDS: usize = 40;
/// A's raw byte records: the raw keyspace is a sibling of the typed store.
const A_RAW_RECORDS: [(&[u8], &[u8]); 2] = [(b"raw/1", b"one"), (b"raw/2", b"two")];
/// Distinct `kind` values; `book` and `bolt` share the prefix `bo`.
const KINDS: [&str; 4] = ["book", "bolt", "pen", "lamp"];
/// The value of every `note` field (3 bytes).
const NOTE: &[u8] = b"xyz";
/// Distinct index terms A holds after phase 0: value and existence terms of
/// kind (4 + 1), price (5 + 1), hot (2 + 1) and $exp (8 + 1).
const A_TERMS: usize = 23;

/// B holds `b/00000`..`b/00004` (7-byte keys) after phase 0 and
/// `b/00005`..`b/10004` on top after phase 1, ids 1..=10005.
const B_INITIAL_RECORDS: usize = 5;
const B_GROWTH_RECORDS: usize = 10_000;
const B_GROWTH_BLOCKS: usize = 5;
const B_TOTAL_RECORDS: usize = B_INITIAL_RECORDS + B_GROWTH_RECORDS;
/// Raw byte records `r/00000`..`r/01999` (7-byte keys, 9-byte values
/// `bee-NNNNN`) published into B in phase 1.
const B_RAW_RECORDS: usize = 2_000;

/// Namespaces created in phase 2, on top of A and B.
const EXTRA_NAMESPACES: usize = 500;
const PHASE0_NAMESPACES: usize = 2;
const PHASE2_NAMESPACES: usize = PHASE0_NAMESPACES + EXTRA_NAMESPACES;

/// The page limit of every unpaged query here.
const MAX_PAGE: usize = Limits::DEFAULT.max_page;
/// The block at which the `$exp` values 1, 2 and 3 expire.
const EXPIRY_BLOCK: BlockNumber = BlockNumber(3);

/// FIXTURE total of a create under `key` with `new_terms` new terms
/// (model 3): `4 + {20 + 4c + b + RowByte n + RootUpdate 3*2} + 2a *
/// {IndexInsert 6 + RootUpdate 2*2} + 30 per new term`, where the stored row
/// is `n = k + b`.
fn create_cost(key: &str, cells: &[(CellName, Cell)], new_terms: u64) -> u64 {
    let k = key.len() as u64;
    let c = cells.len() as u64;
    let a = cells.iter().filter(|(_, cell)| cell.is_attribute()).count() as u64;
    let b: u64 = cells
        .iter()
        .map(|(name, cell)| cell_bytes(name, cell))
        .sum();
    4 + (20 + 4 * c + b + (k + b) + 3 * 2) + 2 * a * (6 + 2 * 2) + 30 * new_terms
}

/// FIXTURE total of a raw put (model 3): `1 + 10 + 2k + v + RootUpdate 2*2`.
fn put_cost(key: &[u8], value: &[u8]) -> u64 {
    1 + 10 + 2 * key.len() as u64 + value.len() as u64 + 2 * 2
}

/// The oracle rule for term newness: a value term is new iff no live record
/// holds an attribute of that name, type and value; an existence term iff
/// none holds that name and type. The fixture blocks only create records, so
/// the set of terms seen so far is exact.
#[derive(Default)]
struct TermLedger {
    /// `(name, type, value)` for a value term, `(name, type, None)` for the
    /// existence term.
    seen: HashSet<(CellName, TypeTag, Option<Value>)>,
}

impl TermLedger {
    /// Counts the terms of `cells` that are new and records them.
    fn admit(&mut self, cells: &[(CellName, Cell)]) -> u64 {
        let mut fresh = 0;
        for (name, cell) in cells {
            if !cell.is_attribute() {
                continue;
            }
            let tag = cell.value.tag();
            let value_term = (name.clone(), tag, Some(cell.value.clone()));
            let existence_term = (name.clone(), tag, None);
            for term in [value_term, existence_term] {
                if self.seen.insert(term) {
                    fresh += 1;
                }
            }
        }
        fresh
    }
}

/// Phase 0 block (model 3): 2 namespace creates at 1 + 50 + 2 = 53; A's 40
/// creates under 4-byte keys, hand-summed: a plain record (price 13 + note
/// 7 + kind 8 bytes, c 3, a 2) costs 4 + (20 + 12 + 28 + RowByte 32 +
/// RootUpdate 6) + 4 * 10 = 142, or 140 for a `pen` (one byte less in b and
/// in n); `hot` (4 bytes, one attribute) adds 4 + 4 + 4 + 20 = 32; `$exp`
/// (12 bytes) adds 4 + 12 + 12 + 20 = 48. Over 40 records, 10 `pen`, 14
/// `hot` and 8 `$exp`: 40 * 142 - 10 * 2 + 14 * 32 + 8 * 48 = 6492, plus 23
/// new terms at 30 -> 7182. 2 raw puts at 1 + 10 + 10 + 3 + 4 = 28; B's 5
/// creates under 7-byte keys: the model-2 totals 551, 263, 262, 313, 233
/// plus RowByte (7 + b) + RootUpdate 2 * (3 + 2 * 2a) each (two index
/// groups per attribute cell): 551 + (74 + 54), 263 + (58 + 38), 262 +
/// (57 + 38), 313 + (62 + 46), 233 + (58 + 38) = 679 + 359 + 357 + 421 +
/// 329 = 2145. Total 106 + 7182 + 56 + 2145.
const PHASE0_BLOCK_COST: u64 = 9489;
/// Phase 1 growth blocks of 2000 B creates each; the totals differ because
/// the `hot`/`$exp` cycles fall differently into each block. Model 2 gave
/// 442 040 for blocks with 667 `hot` records and 442 020 for those with 666
/// (each block holds 500 `pen` and 400 `$exp` records); model 3 adds per
/// record RowByte (7 + b) + RootUpdate 2 * (3 + 2 * 2a), two index groups
/// per attribute cell: 96 for a plain record (b 51, a 4), -1 for a `pen`,
/// +12 with `hot`, +20 with `$exp`, so
/// 2000 * 96 - 500 + 12 * 667 + 20 * 400 = 207 504 (207 492 with 666 `hot`).
const B_GROWTH_BLOCK_COSTS: [u64; B_GROWTH_BLOCKS] = [649_544, 649_512, 649_544, 649_544, 649_512];
/// Phase 1 raw block: 2000 puts at 1 + 10 + 14 + 9 + 4 = 38.
const B_RAW_BLOCK_COST: u64 = 76_000;
/// Phase 2 block: 500 creates at 53.
const PHASE2_BLOCK_COST: u64 = 26_500;
/// What the workload's recorded writes spend: 176 + 312 + 4 + 182 + 282 +
/// 120 + 166 + 4 + 74 + 106 + 4 + 76 + 82 + 110 + (130 + 106 + 106).
const WORKLOAD_BLOCK_SPENT: u64 = 2040;
/// Recorded write receipts of the workload: 12 user writes, 1 host delete,
/// 1 halted expiry delete, 3 expiry deletes (the `Put` on the host path is
/// refused at admission and never recorded).
const WORKLOAD_RECORDED_WRITES: usize = 17;
/// Live records of A at the end of the workload: 40 + 3 created - 1 deleted
/// - 1 host-deleted - 3 expired.
const A_RECORDS_AFTER_WORKLOAD: usize = 38;

fn a_key(i: usize) -> String {
    format!("a/{i:02}")
}

fn b_key(j: usize) -> String {
    format!("b/{j:05}")
}

fn b_raw_key(r: usize) -> Vec<u8> {
    format!("r/{r:05}").into_bytes()
}

fn b_raw_value(r: usize) -> Vec<u8> {
    format!("bee-{r:05}").into_bytes()
}

/// Record `i` of A, submitted out of name order (the engine sorts):
/// `kind` cycles through [`KINDS`] and `price` through -20, -10, 0, 10, 20,
/// so every (kind, price) pair occurs exactly twice among 40 records; `hot`
/// is present on every third record (true on every sixth); `$exp` on every
/// fifth with the values 1..=8; `note` is a field on every record.
fn a_cells(i: usize) -> Vec<(CellName, Cell)> {
    let step = i64::try_from(i % 5).expect("fits");
    let mut cells = vec![
        (name("price"), attr(Value::I64(step * 10 - 20))),
        (name("note"), field(Value::bytes(NOTE))),
        (name("kind"), attr(Value::str(KINDS[i % 4]))),
    ];
    if i.is_multiple_of(3) {
        cells.push((name("hot"), attr(Value::Bool(i.is_multiple_of(6)))));
    }
    if i.is_multiple_of(5) {
        let expiry = u64::try_from(i / 5 + 1).expect("fits");
        cells.push((name("$exp"), attr(Value::U64(expiry))));
    }
    cells
}

/// Record `j` of B: A's layout for index `j` (identical names and values for
/// `j < 40`) plus a distinct `seq` and a distinct `label` term.
fn b_cells(j: usize) -> Vec<(CellName, Cell)> {
    let mut cells = a_cells(j);
    cells.push((
        name("seq"),
        attr(Value::I64(i64::try_from(j).expect("fits"))),
    ));
    cells.push((name("label"), attr(Value::str(&format!("v-{j:05}")))));
    cells
}

/// `a/new1`: every term already exists in A (kind book, price 20, hot true).
fn new1_cells() -> Vec<(CellName, Cell)> {
    vec![
        (name("kind"), attr(Value::str("book"))),
        (name("price"), attr(Value::I64(20))),
        (name("note"), field(Value::bytes(NOTE))),
        (name("hot"), attr(Value::Bool(true))),
    ]
}

/// `a/new2`: kind desk and price 999 are new value terms, `size` is a new
/// name (new value term and new existence term): four new terms.
fn new2_cells() -> Vec<(CellName, Cell)> {
    vec![
        (name("kind"), attr(Value::str("desk"))),
        (name("price"), attr(Value::I64(999))),
        (name("size"), attr(Value::U64(7))),
        (name("note"), field(Value::bytes(NOTE))),
    ]
}

/// `a/new3`: kind sofa, price 777 and size 9 are new value terms; the
/// existence term of `size` exists once `a/new2` is live: three new terms.
fn new3_cells() -> Vec<(CellName, Cell)> {
    vec![
        (name("kind"), attr(Value::str("sofa"))),
        (name("price"), attr(Value::I64(777))),
        (name("size"), attr(Value::U64(9))),
        (name("note"), field(Value::bytes(NOTE))),
    ]
}

fn eq(attribute: &str, value: Value) -> Term {
    Term::eq(name(attribute), value)
}

/// `kind = book`: ten records, ids 1, 5, 9, ..., 37.
fn q_eq_book() -> Query {
    Query::term(eq("kind", Value::str("book")))
}

/// `kind != pen` within Str: the 30 records that are not a pen.
fn q_not_eq_pen() -> Query {
    Query::term(Term::not_eq(name("kind"), Value::str("pen")))
}

/// `-10 <= price < 10`: the value terms -10 and 0, sixteen records.
fn q_range_price() -> Query {
    Query::term(Term::range(
        name("price"),
        Bound::Included(Value::I64(-10)),
        Bound::Excluded(Value::I64(10)),
    ))
}

/// `kind starts with "bo"`: the value terms bolt and book, twenty records.
fn q_starts_with_bo() -> Query {
    Query::term(Term::starts_with(name("kind"), "bo"))
}

/// `hot exists as Bool`: fourteen records.
fn q_exists_hot() -> Query {
    Query::term(Term::exists(name("hot"), TypeTag::Bool))
}

/// `NOT kind = book`: the row map is the source, thirty records match.
fn q_negated_book() -> Query {
    Query::all(vec![Literal::not(eq("kind", Value::str("book")))])
}

/// `kind = book AND price >= 0 AND NOT hot = true`: the book posting is the
/// source, the two further literals are probes.
fn q_and_probes() -> Query {
    Query::all(vec![
        Literal::of(eq("kind", Value::str("book"))),
        Literal::of(Term::range(
            name("price"),
            Bound::Included(Value::I64(0)),
            Bound::Unbounded,
        )),
        Literal::not(eq("hot", Value::Bool(true))),
    ])
}

/// `kind = pen OR price = 20 OR ($exp exists AND hot = false)`.
fn q_or_groups() -> Query {
    Query {
        groups: vec![
            vec![Literal::of(eq("kind", Value::str("pen")))],
            vec![Literal::of(eq("price", Value::I64(20)))],
            vec![
                Literal::of(Term::exists(name("$exp"), TypeTag::U64)),
                Literal::of(eq("hot", Value::Bool(false))),
            ],
        ],
    }
}

/// `price = -20`: eight records, ids 1, 6, 11, ..., 36.
fn q_eq_price_minus_20() -> Query {
    Query::term(eq("price", Value::I64(-20)))
}

/// A value no record holds.
fn q_miss_kind() -> Query {
    Query::term(eq("kind", Value::str("zzz")))
}

/// A name that exists only in B.
fn q_foreign_eq() -> Query {
    Query::term(eq("seq", Value::I64(7)))
}

/// A prefix over a name that exists only in B (a thousand labels there).
fn q_foreign_prefix() -> Query {
    Query::term(Term::starts_with(name("label"), "v-00"))
}

/// A range over a name that exists only in B.
fn q_foreign_range() -> Query {
    Query::term(Term::range(
        name("seq"),
        Bound::Included(Value::I64(0)),
        Bound::Included(Value::I64(1000)),
    ))
}

/// `price != U64(0)`: no record of A holds a U64 price, so the existence
/// posting is missing and nothing is probed.
fn q_not_eq_u64_price() -> Query {
    Query::term(Term::not_eq(name("price"), Value::U64(0)))
}

/// The expiry sweep's own query: `$exp <= 3`.
fn q_expiry() -> Query {
    Query::term(Term::range(
        name("$exp"),
        Bound::Unbounded,
        Bound::Included(Value::U64(EXPIRY_BLOCK.0)),
    ))
}

/// `price >= U64(0)`: only the record whose price was patched to U64.
fn q_range_u64_price() -> Query {
    Query::term(Term::range(
        name("price"),
        Bound::Included(Value::U64(0)),
        Bound::Unbounded,
    ))
}

fn q_eq_sofa() -> Query {
    Query::term(eq("kind", Value::str("sofa")))
}

/// Key plus `kind` and `price`; `zzz` names nothing and costs nothing.
fn named_kind_price_zzz() -> Projection {
    Projection::named(true, [name("kind"), name("price"), name("zzz")])
}

/// No key; `kind` and (where present) `hot`.
fn named_kind_hot() -> Projection {
    Projection::named(false, [name("kind"), name("hot")])
}

/// The first `limit` matches of the reference model with id > `after`, and
/// the id the cursor would carry iff a further match exists.
fn model_page(
    model: &Model,
    query: &Query,
    projection: &Projection,
    limit: usize,
    after: Option<RecordId>,
) -> (Vec<Row>, Option<RecordId>) {
    let mut matches = model.records().filter(|(id, record)| {
        after.is_none_or(|after| *id > after) && query_holds(query, &record.cells)
    });
    let rows: Vec<Row> = matches
        .by_ref()
        .take(limit)
        .map(|(id, record)| project(id, record, projection))
        .collect();
    let next = if matches.next().is_some() {
        rows.last().map(|row| row.id)
    } else {
        None
    };
    (rows, next)
}

/// A's records as published in phase 0.
fn phase0_model() -> Model {
    let mut model = Model::new();
    for i in 0..A_RECORDS {
        let id = model.create(a_key(i), &a_cells(i));
        assert_eq!(id, rid(i as u64 + 1));
    }
    model
}

const GET_ALL: &str = "get_record a/00 ALL";
const GET_ABSENT: &str = "get_record a/99 ALL";
const GET_FOREIGN: &str = "get_record b/00000 (present only in B) ALL";
const GET_NAMED: &str = "get_record a/07 named [kind, price, zzz] with key";
const GET_ID_ONLY: &str = "get_record a/07 ID_ONLY";
const EQ_PAGE1: &str = "Eq(kind, book) ALL limit 4, page 1";
const EQ_PAGE2: &str = "Eq(kind, book) ALL limit 4, page 2";
const EQ_PAGE3: &str = "Eq(kind, book) ALL limit 4, page 3";
const NOT_EQ: &str = "NotEq(kind, pen) ID_ONLY";
const RANGE: &str = "Range(price, [-10, 10)) KEY_ONLY";
const STARTS_WITH: &str = "StartsWith(kind, bo) ID_ONLY";
const EXISTS: &str = "Exists(hot, Bool) ID_ONLY";
const NEGATED: &str = "NOT Eq(kind, book) KEY_ONLY";
const MATCH_ALL: &str = "match-all ID_ONLY";
const AND_PROBES: &str = "Eq(kind, book) AND Range(price >= 0) AND NOT Eq(hot, true) ID_ONLY";
const OR_GROUPS: &str =
    "Eq(kind, pen) OR Eq(price, 20) OR (Exists($exp) AND Eq(hot, false)) KEY_ONLY";
const NAMED_QUERY: &str = "Eq(price, -20) named [kind, hot]";
const COUNT_EQ: &str = "count Eq(kind, book)";
const COUNT_ALL: &str = "count match-all";
const COUNT_OR: &str = "count of the OR query";
const MISS_EQ: &str = "Eq(kind, zzz): value held by no record";
const MISS_FOREIGN_EQ: &str = "Eq(seq, 7): name held only in B";
const MISS_FOREIGN_PREFIX: &str = "StartsWith(label, v-00): name held only in B";
const MISS_FOREIGN_RANGE: &str = "Range(seq, [0, 1000]): name held only in B";
const MISS_NOT_EQ_TYPE: &str = "NotEq(price, U64 0): no U64 price in A";
const ABORT_EQ_PAGE: &str = "Eq(kind, book) ALL limit 4 with budget 100";
const ABORT_NOT_EQ: &str = "NotEq(kind, pen) with budget 30";
const ABORT_RANGE_ENTRIES: &str = "Range(price) with budget 30";
const ABORT_RANGE_TERMS: &str = "Range(price) with budget 14";
const ABORT_NEGATED: &str = "NOT Eq(kind, book) with budget 35";
const ABORT_MATCH_ALL: &str = "match-all with budget 50";
const ABORT_STARTS_WITH: &str = "StartsWith(kind, bo) with budget 22";
const ABORT_EXISTS: &str = "Exists(hot, Bool) with budget 12";
const ABORT_COUNT_ALL: &str = "count match-all with budget 20";
const ABORT_OR: &str = "the OR query with budget 38";
const ABORT_AND: &str = "the AND query with budget 44";
const ABORT_GET: &str = "get_record a/00 ALL with budget 54";
const ABORT_CURSOR_PAGE: &str = "Eq(kind, book) page 2 with budget 60";
const ABORT_NAMED: &str = "Eq(price, -20) named [kind, hot] with budget 40";
const CREATE_EXISTING_TERMS: &str = "create a/new1 (every term exists)";
const CREATE_NEW_TERMS: &str = "create a/new2 (four new terms)";
const CREATE_BOUND_KEY: &str = "create a/00 (key already bound)";
const ABORT_CREATE: &str = "create a/new3 with budget 200";
const CREATE_AFTER_ABORT: &str = "create a/new3 (its terms are still new)";
const PATCH: &str = "patch a/01: set price 20, hot false; remove note, zzz";
const PATCH_TYPE_CHANGE: &str = "patch a/02: price becomes U64 5";
const PATCH_MISSING: &str = "patch a/99";
const ABORT_PATCH: &str = "patch a/05 with budget 80";
const DELETE: &str = "delete a/03";
const DELETE_MISSING: &str = "delete a/03 again";
const ABORT_DELETE: &str = "delete a/06 with budget 80";
const HOST_PUT: &str = "execute_host Put";
const HOST_DELETE: &str = "execute_host delete a/04";
const EXPIRE_HALTED: &str = "expire_records $exp <= 3 with delete budget 110";
const EXPIRE: &str = "expire_records $exp <= 3";
const COUNT_ALL_AFTER: &str = "count match-all after the writes";
const COUNT_EXP_AFTER: &str = "count Exists($exp, U64) after the expiry";
const STALE_CURSOR: &str = "Eq(kind, book) page 2 with the pre-write cursor";
const GET_EXPIRED: &str = "get_record a/00 after the expiry";
const EQ_AFTER: &str = "Eq(kind, book) ID_ONLY after the writes";
const RANGE_U64_AFTER: &str = "Range(price, U64 >= 0) after the type change";
const EQ_NEW_TERM_AFTER: &str = "Eq(kind, sofa) KEY_ONLY after the creates";

/// The read steps, in order; the historical-snapshot test replays these.
const READ_STEPS: usize = 39;

/// The logical result of one workload step.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Observed {
    Record(Option<Row>),
    Page {
        rows: Vec<Row>,
        next: Option<RecordId>,
    },
    Count(u64),
    Applied(Effect),
    Rejected(Rejection),
    Expired(Vec<RecordId>),
    ExpiryHalted {
        applied: Vec<RecordId>,
        reason: Rejection,
    },
}

impl From<Effect> for Observed {
    fn from(effect: Effect) -> Self {
        Self::Applied(effect)
    }
}

impl From<Rejection> for Observed {
    fn from(reason: Rejection) -> Self {
        Self::Rejected(reason)
    }
}

/// A page step; the cursor is handed back for the next page.
fn page_step(
    name: &'static str,
    result: ReadResult<QueryPage>,
) -> (Step<Observed>, Option<Cursor>) {
    let cursor = match &result {
        Ok(metered) => metered.value.next.clone(),
        Err(_) => None,
    };
    let step = read_step(name, result, |page| Observed::Page {
        next: page.next.as_ref().map(Cursor::after),
        rows: page.rows,
    });
    (step, cursor)
}

/// An unpaged query from the start.
fn full_page(
    reader: &impl Reader,
    a: NamespaceId,
    name: &'static str,
    query: &Query,
    projection: &Projection,
    budget: Budget,
) -> Step<Observed> {
    page_step(
        name,
        reader.query(a, query, projection, MAX_PAGE, None, budget),
    )
    .0
}

/// An expiry sweep: one receipt per delete; the host helper exposes no
/// physical trace.
fn expiry_step(
    name: &'static str,
    result: Result<Vec<(RecordId, Receipt)>, ExpiryHalt>,
) -> Step<Observed> {
    match result {
        Ok(deleted) => Step {
            name,
            observed: Observed::Expired(deleted.iter().map(|(id, _)| *id).collect()),
            receipts: deleted.into_iter().map(|(_, receipt)| receipt).collect(),
            physical: None,
        },
        Err(halt) => {
            let mut receipts: Vec<Receipt> = halt
                .applied
                .iter()
                .map(|(_, receipt)| receipt.clone())
                .collect();
            receipts.push(halt.rejected.receipt.clone());
            Step {
                name,
                observed: Observed::ExpiryHalted {
                    applied: halt.applied.iter().map(|(id, _)| *id).collect(),
                    reason: halt.rejected.reason,
                },
                receipts,
                physical: None,
            }
        }
    }
}

/// The read part of the workload plus the first page's cursor.
struct ReadRun {
    steps: Vec<Step<Observed>>,
    first_page_cursor: Option<Cursor>,
}

/// The read part of A's workload, in order, against any reader.
fn read_workload(reader: &impl Reader, a: NamespaceId) -> ReadRun {
    let traced = Budget::unlimited().traced();
    let limited = |units| Budget::limit(units).traced();
    let all = &Projection::ALL;
    let id_only = &Projection::ID_ONLY;
    let key_only = &Projection::KEY_ONLY;
    let mut steps = vec![
        read_step(
            GET_ALL,
            reader.get_record(a, &key(a_key(0)), all, traced),
            Observed::Record,
        ),
        read_step(
            GET_ABSENT,
            reader.get_record(a, &key("a/99"), all, traced),
            Observed::Record,
        ),
        read_step(
            GET_FOREIGN,
            reader.get_record(a, &key(b_key(0)), all, traced),
            Observed::Record,
        ),
        read_step(
            GET_NAMED,
            reader.get_record(a, &key(a_key(7)), &named_kind_price_zzz(), traced),
            Observed::Record,
        ),
        read_step(
            GET_ID_ONLY,
            reader.get_record(a, &key(a_key(7)), id_only, traced),
            Observed::Record,
        ),
    ];
    let (page1, cursor1) = page_step(
        EQ_PAGE1,
        reader.query(a, &q_eq_book(), all, 4, None, traced),
    );
    steps.push(page1);
    let (page2, cursor2) = page_step(
        EQ_PAGE2,
        reader.query(a, &q_eq_book(), all, 4, cursor1.as_ref(), traced),
    );
    steps.push(page2);
    steps.push(
        page_step(
            EQ_PAGE3,
            reader.query(a, &q_eq_book(), all, 4, cursor2.as_ref(), traced),
        )
        .0,
    );
    let pages: [(&str, Query, &Projection); 14] = [
        (NOT_EQ, q_not_eq_pen(), id_only),
        (RANGE, q_range_price(), key_only),
        (STARTS_WITH, q_starts_with_bo(), id_only),
        (EXISTS, q_exists_hot(), id_only),
        (NEGATED, q_negated_book(), key_only),
        (MATCH_ALL, Query::match_all(), id_only),
        (AND_PROBES, q_and_probes(), id_only),
        (OR_GROUPS, q_or_groups(), key_only),
        (NAMED_QUERY, q_eq_price_minus_20(), &named_kind_hot()),
        (MISS_EQ, q_miss_kind(), id_only),
        (MISS_FOREIGN_EQ, q_foreign_eq(), id_only),
        (MISS_FOREIGN_PREFIX, q_foreign_prefix(), id_only),
        (MISS_FOREIGN_RANGE, q_foreign_range(), id_only),
        (MISS_NOT_EQ_TYPE, q_not_eq_u64_price(), id_only),
    ];
    for (step, query, projection) in pages {
        steps.push(full_page(reader, a, step, &query, projection, traced));
    }
    let counts = [
        (COUNT_EQ, q_eq_book()),
        (COUNT_ALL, Query::match_all()),
        (COUNT_OR, q_or_groups()),
    ];
    for (step, query) in counts {
        steps.push(read_step(
            step,
            reader.count(a, &query, traced),
            Observed::Count,
        ));
    }
    // (step, query, projection, page limit, budget).
    let aborted: [(&str, Query, &Projection, usize, u64); 10] = [
        (ABORT_EQ_PAGE, q_eq_book(), all, 4, 100),
        (ABORT_NOT_EQ, q_not_eq_pen(), id_only, MAX_PAGE, 30),
        (ABORT_RANGE_ENTRIES, q_range_price(), key_only, MAX_PAGE, 30),
        (ABORT_RANGE_TERMS, q_range_price(), key_only, MAX_PAGE, 14),
        (ABORT_NEGATED, q_negated_book(), key_only, MAX_PAGE, 35),
        (ABORT_MATCH_ALL, Query::match_all(), id_only, MAX_PAGE, 50),
        (ABORT_STARTS_WITH, q_starts_with_bo(), id_only, MAX_PAGE, 22),
        (ABORT_EXISTS, q_exists_hot(), id_only, MAX_PAGE, 12),
        (ABORT_OR, q_or_groups(), key_only, MAX_PAGE, 38),
        (ABORT_AND, q_and_probes(), id_only, MAX_PAGE, 44),
    ];
    for (step, query, projection, limit, budget) in aborted {
        steps.push(
            page_step(
                step,
                reader.query(a, &query, projection, limit, None, limited(budget)),
            )
            .0,
        );
    }
    steps.push(read_step(
        ABORT_COUNT_ALL,
        reader.count(a, &Query::match_all(), limited(20)),
        Observed::Count,
    ));
    steps.push(read_step(
        ABORT_GET,
        reader.get_record(a, &key(a_key(0)), all, limited(54)),
        Observed::Record,
    ));
    steps.push(
        page_step(
            ABORT_CURSOR_PAGE,
            reader.query(a, &q_eq_book(), all, 4, cursor1.as_ref(), limited(60)),
        )
        .0,
    );
    steps.push(full_page(
        reader,
        a,
        ABORT_NAMED,
        &q_eq_price_minus_20(),
        &named_kind_hot(),
        limited(40),
    ));
    assert_eq!(
        steps.len(),
        READ_STEPS,
        "the read workload has a fixed length"
    );
    ReadRun {
        steps,
        first_page_cursor: cursor1,
    }
}

/// The write part of A's workload plus the reads that observe its effects,
/// on a branch that is dropped afterwards. `stale` is a cursor issued by the
/// branch before any write.
fn write_workload(
    branch: &mut Branch,
    a: NamespaceId,
    stale: Option<&Cursor>,
) -> Vec<Step<Observed>> {
    let traced = Budget::unlimited().traced();
    let limited = |units| Budget::limit(units).traced();
    let expiry_name = name("$exp");
    vec![
        write_step(
            CREATE_EXISTING_TERMS,
            branch.execute(ALICE, create_record(a, "a/new1", new1_cells()), traced),
        ),
        write_step(
            CREATE_NEW_TERMS,
            branch.execute(ALICE, create_record(a, "a/new2", new2_cells()), traced),
        ),
        write_step(
            CREATE_BOUND_KEY,
            branch.execute(ALICE, create_record(a, a_key(0), new1_cells()), traced),
        ),
        write_step(
            ABORT_CREATE,
            branch.execute(
                ALICE,
                create_record(a, "a/new3", new3_cells()),
                limited(200),
            ),
        ),
        write_step(
            CREATE_AFTER_ABORT,
            branch.execute(ALICE, create_record(a, "a/new3", new3_cells()), traced),
        ),
        write_step(
            PATCH,
            branch.execute(
                ALICE,
                patch_record(
                    a,
                    a_key(1),
                    vec![
                        (name("price"), attr(Value::I64(20))),
                        (name("hot"), attr(Value::Bool(false))),
                    ],
                    vec![name("note"), name("zzz")],
                ),
                traced,
            ),
        ),
        write_step(
            PATCH_TYPE_CHANGE,
            branch.execute(
                ALICE,
                patch_record(
                    a,
                    a_key(2),
                    vec![(name("price"), attr(Value::U64(5)))],
                    vec![],
                ),
                traced,
            ),
        ),
        write_step(
            PATCH_MISSING,
            branch.execute(
                ALICE,
                patch_record(
                    a,
                    "a/99",
                    vec![(name("price"), attr(Value::U64(5)))],
                    vec![],
                ),
                traced,
            ),
        ),
        write_step(
            ABORT_PATCH,
            branch.execute(
                ALICE,
                patch_record(
                    a,
                    a_key(5),
                    vec![(name("kind"), attr(Value::str("lamp")))],
                    vec![],
                ),
                limited(80),
            ),
        ),
        write_step(
            DELETE,
            branch.execute(ALICE, delete_record(a, a_key(3)), traced),
        ),
        write_step(
            DELETE_MISSING,
            branch.execute(ALICE, delete_record(a, a_key(3)), traced),
        ),
        write_step(
            ABORT_DELETE,
            branch.execute(ALICE, delete_record(a, a_key(6)), limited(80)),
        ),
        write_step(
            HOST_PUT,
            branch.execute_host(put(a, b"raw/3", b"three"), traced),
        ),
        write_step(
            HOST_DELETE,
            branch.execute_host(delete_record(a, a_key(4)), traced),
        ),
        expiry_step(
            EXPIRE_HALTED,
            expire_records(branch, a, &expiry_name, EXPIRY_BLOCK, traced, limited(110)),
        ),
        expiry_step(
            EXPIRE,
            expire_records(branch, a, &expiry_name, EXPIRY_BLOCK, traced, traced),
        ),
        read_step(
            COUNT_ALL_AFTER,
            branch.count(a, &Query::match_all(), traced),
            Observed::Count,
        ),
        read_step(
            COUNT_EXP_AFTER,
            branch.count(
                a,
                &Query::term(Term::exists(name("$exp"), TypeTag::U64)),
                traced,
            ),
            Observed::Count,
        ),
        page_step(
            STALE_CURSOR,
            branch.query(a, &q_eq_book(), &Projection::ALL, 4, stale, traced),
        )
        .0,
        read_step(
            GET_EXPIRED,
            branch.get_record(a, &key(a_key(0)), &Projection::ALL, traced),
            Observed::Record,
        ),
        full_page(
            branch,
            a,
            EQ_AFTER,
            &q_eq_book(),
            &Projection::ID_ONLY,
            traced,
        ),
        full_page(
            branch,
            a,
            RANGE_U64_AFTER,
            &q_range_u64_price(),
            &Projection::ID_ONLY,
            traced,
        ),
        full_page(
            branch,
            a,
            EQ_NEW_TERM_AFTER,
            &q_eq_sofa(),
            &Projection::KEY_ONLY,
            traced,
        ),
    ]
}

/// Runs the whole workload on a branch from the current head and drops the
/// branch, so the published state is exactly what it was before.
fn run_workload(world: &World) -> Vec<Step<Observed>> {
    let head = world.engine.head();
    let mut branch = begin(&world.engine, None);
    let b_typed_before = branch.typed_record_count(world.b);
    let read = read_workload(&branch, world.a);
    let mut steps = read.steps;
    steps.extend(write_workload(
        &mut branch,
        world.a,
        read.first_page_cursor.as_ref(),
    ));
    assert_eq!(
        branch.spent(),
        Cost::new(WORKLOAD_BLOCK_SPENT),
        "the recorded writes of the workload"
    );
    assert_eq!(branch.receipts().len(), WORKLOAD_RECORDED_WRITES);
    assert_eq!(
        branch.typed_record_count(world.a),
        Some(A_RECORDS_AFTER_WORKLOAD),
        "A on the branch after the workload"
    );
    assert_eq!(
        branch.index_consistent(world.a),
        Some(true),
        "A's index on the branch is exactly derived from its rows"
    );
    assert_eq!(
        branch.typed_record_count(world.b),
        b_typed_before,
        "A's workload never touches B"
    );
    drop(branch);
    assert_eq!(
        world.engine.head(),
        head,
        "a dropped branch never publishes"
    );
    steps
}

fn page_observed(
    model: &Model,
    query: &Query,
    projection: &Projection,
    limit: usize,
    after: Option<RecordId>,
) -> Observed {
    let (rows, next) = model_page(model, query, projection, limit, after);
    Observed::Page { rows, next }
}

/// Every receipt and result of the workload, derived by hand from the
/// charge points in docs/metering.md and the fixture table
/// (id = i + 1):
///
/// | i  | kind | price | hot   | $exp | cells | ALL bytes |
/// |----|------|-------|-------|------|-------|-----------|
/// | 0  | book | -20   | true  | 1    | 5     | 48        |
/// | 1  | bolt | -10   |       |      | 3     | 32        |
/// | 2  | pen  | 0     |       |      | 3     | 31        |
/// | 3  | lamp | 10    | false |      | 4     | 36        |
/// | 4  | book | 20    |       |      | 3     | 32        |
/// | 5  | bolt | -20   |       | 2    | 4     | 44        |
/// | 6  | pen  | -10   | true  |      | 4     | 35        |
/// | 7  | lamp | 0     |       |      | 3     | 32        |
/// | .. | (period 4, 5, 3 and 5 respectively)                |
///
/// ALL bytes = key 4 + kind (4 + 4, or 4 + 3 for pen) + price (5 + 8) +
/// note (4 + 3) + hot (3 + 1 when present) + $exp (4 + 8 when present).
fn expected_baseline() -> Vec<Expected<Observed>> {
    let mut model = phase0_model();
    let mut expected = Vec::new();
    let mut push = |name, observed, receipts| {
        expected.push(Expected {
            name,
            observed,
            receipts,
        });
    };
    expected_point_reads(&model, &mut push);
    expected_cursor_walk(&model, &mut push);
    expected_operator_queries(&model, &mut push);
    expected_misses(&mut push);
    expected_counts(&model, &mut push);
    expected_aborted_reads(&mut push);
    expected_writes(&mut model, &mut push);
    expected_reads_after_writes(&model, &mut push);
    expected
}

/// Where a section of the baseline appends its steps.
type Push<'a> = &'a mut dyn FnMut(&'static str, Observed, Vec<Receipt>);

/// A `get_record` that finds no record.
fn get_miss_receipt() -> Receipt {
    itemized(read_applied(4), &[(DirectoryLookup, 1), (PointRead, 1)])
}

/// Point reads.
fn expected_point_reads(model: &Model, push: Push) {
    let all = &Projection::ALL;
    let id_only = &Projection::ID_ONLY;
    // a/00 under ALL: 1 + 3 + {3 + 48}.
    push(
        GET_ALL,
        Observed::Record(model.row(a_key(0), all)),
        vec![itemized(
            read_applied(55),
            &[
                (DirectoryLookup, 1),
                (PointRead, 1),
                (ResultRow, 1),
                (ReadByte, 48),
            ],
        )],
    );
    push(GET_ABSENT, Observed::Record(None), vec![get_miss_receipt()]);
    push(
        GET_FOREIGN,
        Observed::Record(None),
        vec![get_miss_receipt()],
    );
    // a/07 (lamp, price 0, no hot): key 4 + kind 8 + price 13 = 25; zzz
    // costs nothing.
    push(
        GET_NAMED,
        Observed::Record(model.row(a_key(7), &named_kind_price_zzz())),
        vec![itemized(
            read_applied(32),
            &[
                (DirectoryLookup, 1),
                (PointRead, 1),
                (ResultRow, 1),
                (ReadByte, 25),
            ],
        )],
    );
    push(
        GET_ID_ONLY,
        Observed::Record(model.row(a_key(7), id_only)),
        vec![itemized(
            read_applied(7),
            &[(DirectoryLookup, 1), (PointRead, 1), (ResultRow, 1)],
        )],
    );
}

/// The cursor walk over the book posting (ids 1, 5, 9, ..., 37).
fn expected_cursor_walk(model: &Model, push: Push) {
    let all = &Projection::ALL;
    // Page 1: PostingEntry for 1, 5, 9, 13 and the head 17 beyond the page;
    // rows 48 + 32 + 32 + 36 = 148 bytes. 1 + 5 + 4 + 5 + 12 + 148.
    let (rows1, next1) = model_page(model, &q_eq_book(), all, 4, None);
    assert_eq!(next1, Some(rid(13)));
    push(
        EQ_PAGE1,
        Observed::Page {
            rows: rows1,
            next: next1,
        },
        vec![itemized(
            read_applied(175),
            &[
                (DirectoryLookup, 1),
                (QueryGroup, 1),
                (TermSeek, 1),
                (PostingEntry, 5),
                (ResultRow, 4),
                (ReadByte, 148),
            ],
        )],
    );
    // Page 2 after 13: entries 17, 21, 25, 29 and the head 33; rows 32 + 44
    // + 36 + 32 = 144. 1 + 5 + 4 + 5 + 12 + 144.
    let (rows2, next2) = model_page(model, &q_eq_book(), all, 4, next1);
    assert_eq!(next2, Some(rid(29)));
    push(
        EQ_PAGE2,
        Observed::Page {
            rows: rows2,
            next: next2,
        },
        vec![itemized(
            read_applied(171),
            &[
                (DirectoryLookup, 1),
                (QueryGroup, 1),
                (TermSeek, 1),
                (PostingEntry, 5),
                (ResultRow, 4),
                (ReadByte, 144),
            ],
        )],
    );
    // Page 3 after 29: exactly the remaining entries 33, 37 (32 + 36 bytes),
    // then the posting is exhausted without a charge. 1 + 5 + 4 + 2 + 6 + 68.
    let (rows3, next3) = model_page(model, &q_eq_book(), all, 4, next2);
    assert_eq!(next3, None);
    push(
        EQ_PAGE3,
        Observed::Page {
            rows: rows3,
            next: next3,
        },
        vec![itemized(
            read_applied(86),
            &[
                (DirectoryLookup, 1),
                (QueryGroup, 1),
                (TermSeek, 1),
                (PostingEntry, 2),
                (ResultRow, 2),
                (ReadByte, 68),
            ],
        )],
    );
}

/// One query per operator.
fn expected_operator_queries(model: &Model, push: Push) {
    let id_only = &Projection::ID_ONLY;
    let key_only = &Projection::KEY_ONLY;
    // NotEq: the existence posting of kind:Str holds all 40 ids; every entry
    // is probed by the NotEq literal itself; 30 pass. 1 + 5 + 4 + 40 + 80 + 90.
    push(
        NOT_EQ,
        page_observed(model, &q_not_eq_pen(), id_only, MAX_PAGE, None),
        vec![itemized(
            read_applied(220),
            &[
                (DirectoryLookup, 1),
                (QueryGroup, 1),
                (TermSeek, 1),
                (PostingEntry, 40),
                (RecordProbe, 40),
                (ResultRow, 30),
            ],
        )],
    );
    // Range [-10, 10): the value terms -10 and 0 are visited (10 terminates
    // the run unpaid); 16 entries, 16 rows of 4 key bytes.
    // 1 + 5 + 4 + 6 + 16 + 48 + 64.
    push(
        RANGE,
        page_observed(model, &q_range_price(), key_only, MAX_PAGE, None),
        vec![itemized(
            read_applied(144),
            &[
                (DirectoryLookup, 1),
                (QueryGroup, 1),
                (TermSeek, 1),
                (TermVisit, 2),
                (PostingEntry, 16),
                (ResultRow, 16),
                (ReadByte, 64),
            ],
        )],
    );
    // StartsWith "bo": bolt then book are visited, lamp ends the run; 20
    // entries. 1 + 5 + 4 + 6 + 20 + 60.
    push(
        STARTS_WITH,
        page_observed(model, &q_starts_with_bo(), id_only, MAX_PAGE, None),
        vec![itemized(
            read_applied(96),
            &[
                (DirectoryLookup, 1),
                (QueryGroup, 1),
                (TermSeek, 1),
                (TermVisit, 2),
                (PostingEntry, 20),
                (ResultRow, 20),
            ],
        )],
    );
    // Exists(hot, Bool): the existence posting of 14 ids. 1 + 5 + 4 + 14 + 42.
    push(
        EXISTS,
        page_observed(model, &q_exists_hot(), id_only, MAX_PAGE, None),
        vec![itemized(
            read_applied(66),
            &[
                (DirectoryLookup, 1),
                (QueryGroup, 1),
                (TermSeek, 1),
                (PostingEntry, 14),
                (ResultRow, 14),
            ],
        )],
    );
    // Negated: every row is visited and probed, 30 pass, 4 key bytes each.
    // 1 + 5 + 40 + 80 + 90 + 120.
    push(
        NEGATED,
        page_observed(model, &q_negated_book(), key_only, MAX_PAGE, None),
        vec![itemized(
            read_applied(336),
            &[
                (DirectoryLookup, 1),
                (QueryGroup, 1),
                (RowVisit, 40),
                (RecordProbe, 40),
                (ResultRow, 30),
                (ReadByte, 120),
            ],
        )],
    );
    // Match-all: 40 row visits, 40 rows. 1 + 5 + 40 + 120.
    push(
        MATCH_ALL,
        page_observed(model, &Query::match_all(), id_only, MAX_PAGE, None),
        vec![itemized(
            read_applied(166),
            &[
                (DirectoryLookup, 1),
                (QueryGroup, 1),
                (RowVisit, 40),
                (ResultRow, 40),
            ],
        )],
    );
    // AND: the 10 book entries; the price probe fails for i = 0, 16, 20, 36
    // (1 probe each) and passes for the other six, of which i = 12, 24 fail
    // the hot probe (2 probes each): 4 + 12 = 16 probes, 4 rows (ids 5, 9,
    // 29, 33). 1 + 5 + 4 + 10 + 32 + 12.
    push(
        AND_PROBES,
        page_observed(model, &q_and_probes(), id_only, MAX_PAGE, None),
        vec![itemized(
            read_applied(64),
            &[
                (DirectoryLookup, 1),
                (QueryGroup, 1),
                (TermSeek, 1),
                (PostingEntry, 10),
                (RecordProbe, 16),
                (ResultRow, 4),
            ],
        )],
    );
    // OR: three groups opened (15 + 12); the pen posting (10 entries) and
    // the price-20 posting (8) need no probe; the $exp posting (8) is probed
    // by Eq(hot, false) and only id 16 passes; 17 distinct rows of 4 key
    // bytes. 1 + 15 + 12 + 26 + 16 + 51 + 68.
    push(
        OR_GROUPS,
        page_observed(model, &q_or_groups(), key_only, MAX_PAGE, None),
        vec![itemized(
            read_applied(189),
            &[
                (DirectoryLookup, 1),
                (QueryGroup, 3),
                (TermSeek, 3),
                (PostingEntry, 26),
                (RecordProbe, 8),
                (ResultRow, 17),
                (ReadByte, 68),
            ],
        )],
    );
    // Named [kind, hot] over price -20 (i = 0, 5, ..., 35): kind 8 (7 for a
    // pen) plus hot 4 where present: 12 + 8 + 7 + 12 + 8 + 8 + 11 + 8 = 74.
    // 1 + 5 + 4 + 8 + 24 + 74.
    push(
        NAMED_QUERY,
        page_observed(
            model,
            &q_eq_price_minus_20(),
            &named_kind_hot(),
            MAX_PAGE,
            None,
        ),
        vec![itemized(
            read_applied(116),
            &[
                (DirectoryLookup, 1),
                (QueryGroup, 1),
                (TermSeek, 1),
                (PostingEntry, 8),
                (ResultRow, 8),
                (ReadByte, 74),
            ],
        )],
    );
}

/// Misses: a missing term or an empty run costs the seek only, whether the
/// term is held by no one or only by B.
fn expected_misses(push: Push) {
    let empty_page = Observed::Page {
        rows: Vec::new(),
        next: None,
    };
    let miss_receipt = itemized(
        read_applied(10),
        &[(DirectoryLookup, 1), (QueryGroup, 1), (TermSeek, 1)],
    );
    for step in [
        MISS_EQ,
        MISS_FOREIGN_EQ,
        MISS_FOREIGN_PREFIX,
        MISS_FOREIGN_RANGE,
        MISS_NOT_EQ_TYPE,
    ] {
        push(step, empty_page.clone(), vec![miss_receipt.clone()]);
    }
}

/// Counts: the same enumeration without row charges.
fn expected_counts(model: &Model, push: Push) {
    push(
        COUNT_EQ,
        Observed::Count(model.ids(&q_eq_book()).len() as u64),
        vec![itemized(
            read_applied(20),
            &[
                (DirectoryLookup, 1),
                (QueryGroup, 1),
                (TermSeek, 1),
                (PostingEntry, 10),
            ],
        )],
    );
    push(
        COUNT_ALL,
        Observed::Count(model.ids(&Query::match_all()).len() as u64),
        vec![itemized(
            read_applied(46),
            &[(DirectoryLookup, 1), (QueryGroup, 1), (RowVisit, 40)],
        )],
    );
    push(
        COUNT_OR,
        Observed::Count(model.ids(&q_or_groups()).len() as u64),
        vec![itemized(
            read_applied(70),
            &[
                (DirectoryLookup, 1),
                (QueryGroup, 3),
                (TermSeek, 3),
                (PostingEntry, 26),
                (RecordProbe, 8),
            ],
        )],
    );
}

/// Aborted enumerations: every per-entry class is charged just before the
/// entry is used, so the abort point is exact and no partial page is
/// returned.
fn expected_aborted_reads(push: Push) {
    let out_of_budget = Observed::Rejected(Rejection::OutOfBudget);
    // Page 1 with budget 100: charge points 1, 6, 10, 11 (entry 1), 62 (row
    // 1: 3 + 48), 63, 98 (row 5), 99 (entry 9); row 9 (35) would reach 134.
    push(
        ABORT_EQ_PAGE,
        out_of_budget.clone(),
        vec![itemized(
            read_rejected(99, Rejection::OutOfBudget),
            &[
                (DirectoryLookup, 1),
                (QueryGroup, 1),
                (TermSeek, 1),
                (PostingEntry, 3),
                (ResultRow, 2),
                (ReadByte, 80),
            ],
        )],
    );
    // NotEq with budget 30: 10; id 1: 11, 13, 16; id 2: 17, 19, 22; id 3
    // (a pen) 23, 25; id 4: 26, 28; its row (3) would reach 31.
    push(
        ABORT_NOT_EQ,
        out_of_budget.clone(),
        vec![itemized(
            read_rejected(28, Rejection::OutOfBudget),
            &[
                (DirectoryLookup, 1),
                (QueryGroup, 1),
                (TermSeek, 1),
                (PostingEntry, 4),
                (RecordProbe, 4),
                (ResultRow, 2),
            ],
        )],
    );
    // Range with budget 30: 10, terms 13, 16; entry 17, row 24 (3 + 4);
    // entry 25; the next row (7) would reach 32.
    push(
        ABORT_RANGE_ENTRIES,
        out_of_budget.clone(),
        vec![itemized(
            read_rejected(25, Rejection::OutOfBudget),
            &[
                (DirectoryLookup, 1),
                (QueryGroup, 1),
                (TermSeek, 1),
                (TermVisit, 2),
                (PostingEntry, 2),
                (ResultRow, 1),
                (ReadByte, 4),
            ],
        )],
    );
    // Range with budget 14: the first term visit reaches 13, the second 16.
    push(
        ABORT_RANGE_TERMS,
        out_of_budget.clone(),
        vec![itemized(
            read_rejected(13, Rejection::OutOfBudget),
            &[
                (DirectoryLookup, 1),
                (QueryGroup, 1),
                (TermSeek, 1),
                (TermVisit, 1),
            ],
        )],
    );
    // Negated with budget 35: 6; id 1 (a book): 7, 9; id 2: 10, 12, row 19;
    // id 3: 20, 22, row 29; id 4: 30, 32; its row (7) would reach 39.
    push(
        ABORT_NEGATED,
        out_of_budget.clone(),
        vec![itemized(
            read_rejected(32, Rejection::OutOfBudget),
            &[
                (DirectoryLookup, 1),
                (QueryGroup, 1),
                (RowVisit, 4),
                (RecordProbe, 4),
                (ResultRow, 2),
                (ReadByte, 8),
            ],
        )],
    );
    // Match-all with budget 50: 6 + 4 per row reaches exactly 50 after row
    // 11; the twelfth row visit would reach 51.
    push(
        ABORT_MATCH_ALL,
        out_of_budget.clone(),
        vec![itemized(
            read_rejected(50, Rejection::OutOfBudget),
            &[
                (DirectoryLookup, 1),
                (QueryGroup, 1),
                (RowVisit, 11),
                (ResultRow, 11),
            ],
        )],
    );
    // StartsWith with budget 22: 10, terms 13, 16; entry 17, row 20, entry
    // 21; the next row would reach 24.
    push(
        ABORT_STARTS_WITH,
        out_of_budget.clone(),
        vec![itemized(
            read_rejected(21, Rejection::OutOfBudget),
            &[
                (DirectoryLookup, 1),
                (QueryGroup, 1),
                (TermSeek, 1),
                (TermVisit, 2),
                (PostingEntry, 2),
                (ResultRow, 1),
            ],
        )],
    );
    // Exists with budget 12: 10, entry 11; the first row would reach 14.
    push(
        ABORT_EXISTS,
        out_of_budget.clone(),
        vec![itemized(
            read_rejected(11, Rejection::OutOfBudget),
            &[
                (DirectoryLookup, 1),
                (QueryGroup, 1),
                (TermSeek, 1),
                (PostingEntry, 1),
            ],
        )],
    );
    // OR with budget 38: groups 6, 10, 15, 19, 24, 28; first heads: pen 29,
    // price-20 30, $exp 31 + probe 33 (id 1 fails), 34 + 36 (id 6 fails),
    // 37; the probe of id 11 would reach 39.
    push(
        ABORT_OR,
        out_of_budget.clone(),
        vec![itemized(
            read_rejected(37, Rejection::OutOfBudget),
            &[
                (DirectoryLookup, 1),
                (QueryGroup, 3),
                (TermSeek, 3),
                (PostingEntry, 5),
                (RecordProbe, 2),
            ],
        )],
    );
    // AND with budget 44: 10; id 1: 11, 13 (fails); id 5: 14, 16, 18, row
    // 21; id 9: 22, 24, 26, row 29; id 13: 30, 32, 34 (fails hot); id 17:
    // 35, 37; id 21: 38, 40; id 25: 41, 43; its second probe would reach 45.
    push(
        ABORT_AND,
        out_of_budget.clone(),
        vec![itemized(
            read_rejected(43, Rejection::OutOfBudget),
            &[
                (DirectoryLookup, 1),
                (QueryGroup, 1),
                (TermSeek, 1),
                (PostingEntry, 7),
                (RecordProbe, 10),
                (ResultRow, 2),
            ],
        )],
    );
    // count match-all with budget 20: 6 + 14 row visits; the fifteenth would
    // reach 21.
    push(
        ABORT_COUNT_ALL,
        out_of_budget.clone(),
        vec![itemized(
            read_rejected(20, Rejection::OutOfBudget),
            &[(DirectoryLookup, 1), (QueryGroup, 1), (RowVisit, 14)],
        )],
    );
    // get_record with budget 54: the 51-unit row group does not fit after 4.
    push(
        ABORT_GET,
        out_of_budget.clone(),
        vec![itemized(
            read_rejected(4, Rejection::OutOfBudget),
            &[(DirectoryLookup, 1), (PointRead, 1)],
        )],
    );
    // Page 2 with budget 60: 10, entry 11, row 17 (3 + 32) -> 46, entry 47;
    // row 21 (3 + 44) would reach 94.
    push(
        ABORT_CURSOR_PAGE,
        out_of_budget.clone(),
        vec![itemized(
            read_rejected(47, Rejection::OutOfBudget),
            &[
                (DirectoryLookup, 1),
                (QueryGroup, 1),
                (TermSeek, 1),
                (PostingEntry, 2),
                (ResultRow, 1),
                (ReadByte, 32),
            ],
        )],
    );
    // Named with budget 40: 10, entry 11, row 1 (3 + 12) -> 26, entry 27,
    // row 6 (3 + 8) -> 38, entry 39; row 11 (3 + 7) would reach 49.
    push(
        ABORT_NAMED,
        out_of_budget.clone(),
        vec![itemized(
            read_rejected(39, Rejection::OutOfBudget),
            &[
                (DirectoryLookup, 1),
                (QueryGroup, 1),
                (TermSeek, 1),
                (PostingEntry, 3),
                (ResultRow, 2),
                (ReadByte, 20),
            ],
        )],
    );
}

/// The writes, applied to the model as they go.
fn expected_writes(model: &mut Model, push: Push) {
    let out_of_budget = Observed::Rejected(Rejection::OutOfBudget);
    // a/new1: c 4, a 3, b = hot 4 + kind 8 + note 7 + price 13 = 32, row n =
    // key 6 + 32 = 38, no new term: 4 + (20 + 16 + 32 + 38 + RootUpdate 3*2)
    // + 6 * (IndexInsert 6 + RootUpdate 2*2) = 4 + 112 + 60 = 176.
    let new1 = model.create("a/new1", &new1_cells());
    push(
        CREATE_EXISTING_TERMS,
        Observed::Applied(Effect::RecordCreated { id: new1 }),
        vec![itemized(
            applied(176),
            &[
                (DirectoryLookup, 1),
                (PointRead, 1),
                (RecordCreate, 1),
                (CellWrite, 4),
                (CellByte, 32),
                (RowByte, 38),
                (RootUpdate, 3 + 6 * 2),
                (IndexInsert, 6),
            ],
        )],
    );
    // a/new2: c 4, a 3, b = kind 8 + note 7 + price 13 + size 12 = 40, n =
    // 6 + 40 = 46, four new terms: 4 + (20 + 16 + 40 + 46 + 6) + 60 + 120 =
    // 312.
    let new2 = model.create("a/new2", &new2_cells());
    push(
        CREATE_NEW_TERMS,
        Observed::Applied(Effect::RecordCreated { id: new2 }),
        vec![itemized(
            applied(312),
            &[
                (DirectoryLookup, 1),
                (PointRead, 1),
                (RecordCreate, 1),
                (CellWrite, 4),
                (CellByte, 40),
                (RowByte, 46),
                (RootUpdate, 3 + 6 * 2),
                (IndexInsert, 6),
                (IndexTermCreate, 4),
            ],
        )],
    );
    push(
        CREATE_BOUND_KEY,
        Observed::Rejected(Rejection::AlreadyExists),
        vec![itemized(
            rejected(4, Rejection::AlreadyExists),
            &[(DirectoryLookup, 1), (PointRead, 1)],
        )],
    );
    // a/new3 with budget 200: 4, record group {20 + 16 + 40 + RowByte 46 +
    // RootUpdate 6} = 128 -> 132, kind sofa (new, 40) 172, its existence
    // (10) 182; price 777 (new, 40) would reach 222. Nothing is kept.
    push(
        ABORT_CREATE,
        out_of_budget.clone(),
        vec![itemized(
            rejected(182, Rejection::OutOfBudget),
            &[
                (DirectoryLookup, 1),
                (PointRead, 1),
                (RecordCreate, 1),
                (CellWrite, 4),
                (CellByte, 40),
                (RowByte, 46),
                (RootUpdate, 3 + 2 * 2),
                (IndexInsert, 2),
                (IndexTermCreate, 1),
            ],
        )],
    );
    // a/new3 again: the aborted create left no term behind, so sofa, 777 and
    // size 9 are still new (the existence of size is not): 4 + 128 + 60 + 90
    // = 282.
    let new3 = model.create("a/new3", &new3_cells());
    push(
        CREATE_AFTER_ABORT,
        Observed::Applied(Effect::RecordCreated { id: new3 }),
        vec![itemized(
            applied(282),
            &[
                (DirectoryLookup, 1),
                (PointRead, 1),
                (RecordCreate, 1),
                (CellWrite, 4),
                (CellByte, 40),
                (RowByte, 46),
                (RootUpdate, 3 + 6 * 2),
                (IndexInsert, 6),
                (IndexTermCreate, 3),
            ],
        )],
    );
    // a/01 (bolt, -10, note): s 2, b = price 13 + hot 4 = 17, r 2; the row
    // after the patch is key 4 + hot 4 + kind 8 + price 13 = 29 -> group
    // {10 + 8 + 17 + 8 + 29 + RootUpdate 4} = 76; then hot (absent ->
    // false): two inserts of existing terms at 10; note: a field, nothing;
    // price: value -10 removed (10), value 20 inserted (10), the existence
    // term untouched; zzz: nothing. 4 + 76 + 20 + 10 + 10 = 120.
    let patched = model.patch(
        a_key(1),
        &[
            (name("price"), attr(Value::I64(20))),
            (name("hot"), attr(Value::Bool(false))),
        ],
        &[name("note"), name("zzz")],
    );
    push(
        PATCH,
        Observed::Applied(Effect::RecordPatched { id: patched }),
        vec![itemized(
            applied(120),
            &[
                (DirectoryLookup, 1),
                (PointRead, 1),
                (RecordPatch, 1),
                (CellWrite, 2),
                (CellByte, 17),
                (CellRemove, 2),
                (RowByte, 29),
                (RootUpdate, 2 + 4 * 2),
                (IndexInsert, 3),
                (IndexRemove, 1),
            ],
        )],
    );
    // a/02 (pen, price I64 0): s 1, b 13, row n = 4 + kind 7 + note 7 +
    // price 13 = 31 -> group {10 + 4 + 13 + 31 + 4} = 62; the I64 value and
    // existence terms are removed (10 each), the U64 value and existence
    // terms are new (40 each). 4 + 62 + 20 + 80 = 166.
    let retyped = model.patch(a_key(2), &[(name("price"), attr(Value::U64(5)))], &[]);
    push(
        PATCH_TYPE_CHANGE,
        Observed::Applied(Effect::RecordPatched { id: retyped }),
        vec![itemized(
            applied(166),
            &[
                (DirectoryLookup, 1),
                (PointRead, 1),
                (RecordPatch, 1),
                (CellWrite, 1),
                (CellByte, 13),
                (RowByte, 31),
                (RootUpdate, 2 + 4 * 2),
                (IndexRemove, 2),
                (IndexInsert, 2),
                (IndexTermCreate, 2),
            ],
        )],
    );
    push(
        PATCH_MISSING,
        Observed::Rejected(Rejection::NotFound),
        vec![itemized(
            rejected(4, Rejection::NotFound),
            &[(DirectoryLookup, 1), (PointRead, 1)],
        )],
    );
    // a/05 ($exp 2, bolt, note, price) with budget 80: the row after the
    // patch is key 4 + $exp 12 + kind lamp 8 + note 7 + price 13 = 44, so 4 +
    // {10 + 4 + 8 + 44 + RootUpdate 4} = 74; removing bolt (10) would reach
    // 84.
    push(
        ABORT_PATCH,
        out_of_budget.clone(),
        vec![itemized(
            rejected(74, Rejection::OutOfBudget),
            &[
                (DirectoryLookup, 1),
                (PointRead, 1),
                (RecordPatch, 1),
                (CellWrite, 1),
                (CellByte, 8),
                (RowByte, 44),
                (RootUpdate, 2),
            ],
        )],
    );
    // a/03 (hot, lamp, note, price): 4 cells, 3 attributes: 4 + (20 + 16 +
    // RootUpdate 6) + 6 * (IndexRemove 6 + RootUpdate 4) = 106.
    let deleted = model.delete(a_key(3));
    push(
        DELETE,
        Observed::Applied(Effect::RecordDeleted { id: deleted }),
        vec![itemized(
            applied(106),
            &[
                (DirectoryLookup, 1),
                (PointRead, 1),
                (RecordDelete, 1),
                (CellRemove, 4),
                (RootUpdate, 3 + 6 * 2),
                (IndexRemove, 6),
            ],
        )],
    );
    push(
        DELETE_MISSING,
        Observed::Rejected(Rejection::NotFound),
        vec![itemized(
            rejected(4, Rejection::NotFound),
            &[(DirectoryLookup, 1), (PointRead, 1)],
        )],
    );
    // a/06 (hot, pen, note, price) with budget 80: 4, group {20 + 16 + 6}
    // -> 46, three removes at 10 -> 56, 66, 76; the fourth would reach 86.
    // Nothing is removed.
    push(
        ABORT_DELETE,
        out_of_budget,
        vec![itemized(
            rejected(76, Rejection::OutOfBudget),
            &[
                (DirectoryLookup, 1),
                (PointRead, 1),
                (RecordDelete, 1),
                (CellRemove, 4),
                (RootUpdate, 3 + 3 * 2),
                (IndexRemove, 3),
            ],
        )],
    );
    // The host path refuses a raw put at admission: spent 0, not recorded.
    let not_host = Rejection::InvalidArgument(ArgumentFault::NotHostOperation);
    push(
        HOST_PUT,
        Observed::Rejected(not_host),
        vec![itemized(rejected(0, not_host), &[])],
    );
    // a/04 (book, note, price 20): 3 cells, 2 attributes: 4 + (20 + 12 + 6)
    // + 4 * 10 = 82.
    let host_deleted = model.delete(a_key(4));
    push(
        HOST_DELETE,
        Observed::Applied(Effect::RecordDeleted { id: host_deleted }),
        vec![itemized(
            applied(82),
            &[
                (DirectoryLookup, 1),
                (PointRead, 1),
                (RecordDelete, 1),
                (CellRemove, 3),
                (RootUpdate, 3 + 4 * 2),
                (IndexRemove, 4),
            ],
        )],
    );
    // Expiry at block 3: the sweep's page is ids 1, 6, 11 ($exp 1, 2, 3).
    let expiring = model.ids(&q_expiry());
    assert_eq!(expiring, vec![rid(1), rid(6), rid(11)]);
    // With a delete budget of 110 the first delete (a/00: 5 cells, 4
    // attributes) charges 4, {20 + 20 + 6} -> 50 and six removes at 10 ->
    // 110; the seventh would reach 120. The halt applies nothing; the
    // rejected delete is recorded with its 110 units.
    push(
        EXPIRE_HALTED,
        Observed::ExpiryHalted {
            applied: Vec::new(),
            reason: Rejection::OutOfBudget,
        },
        vec![itemized(
            rejected(110, Rejection::OutOfBudget),
            &[
                (DirectoryLookup, 1),
                (PointRead, 1),
                (RecordDelete, 1),
                (CellRemove, 5),
                (RootUpdate, 3 + 6 * 2),
                (IndexRemove, 6),
            ],
        )],
    );
    // The full sweep: a/00 (5 cells, 4 attributes) 4 + (20 + 20 + 6) + 8 *
    // 10 = 130; a/05 and a/10 ($exp, kind, note, price) 4 + (20 + 16 + 6) +
    // 6 * 10 = 106 each.
    for id in &expiring {
        let key = model.record(*id).expect("a live record").key.clone();
        model.delete(key.as_bytes());
    }
    push(
        EXPIRE,
        Observed::Expired(expiring),
        vec![
            itemized(
                applied(130),
                &[
                    (DirectoryLookup, 1),
                    (PointRead, 1),
                    (RecordDelete, 1),
                    (CellRemove, 5),
                    (RootUpdate, 3 + 8 * 2),
                    (IndexRemove, 8),
                ],
            ),
            itemized(
                applied(106),
                &[
                    (DirectoryLookup, 1),
                    (PointRead, 1),
                    (RecordDelete, 1),
                    (CellRemove, 4),
                    (RootUpdate, 3 + 6 * 2),
                    (IndexRemove, 6),
                ],
            ),
            itemized(
                applied(106),
                &[
                    (DirectoryLookup, 1),
                    (PointRead, 1),
                    (RecordDelete, 1),
                    (CellRemove, 4),
                    (RootUpdate, 3 + 6 * 2),
                    (IndexRemove, 6),
                ],
            ),
        ],
    );
}

/// Reads after the writes, on the branch.
fn expected_reads_after_writes(model: &Model, push: Push) {
    let id_only = &Projection::ID_ONLY;
    let key_only = &Projection::KEY_ONLY;
    assert_eq!(model.len(), A_RECORDS_AFTER_WORKLOAD);
    push(
        COUNT_ALL_AFTER,
        Observed::Count(model.ids(&Query::match_all()).len() as u64),
        vec![itemized(
            read_applied(44),
            &[(DirectoryLookup, 1), (QueryGroup, 1), (RowVisit, 38)],
        )],
    );
    push(
        COUNT_EXP_AFTER,
        Observed::Count(
            model
                .ids(&Query::term(Term::exists(name("$exp"), TypeTag::U64)))
                .len() as u64,
        ),
        vec![itemized(
            read_applied(15),
            &[
                (DirectoryLookup, 1),
                (QueryGroup, 1),
                (TermSeek, 1),
                (PostingEntry, 5),
            ],
        )],
    );
    // The branch's mutation serial advanced, so the pre-write cursor is
    // refused at admission: spent 0, receipt issued.
    push(
        STALE_CURSOR,
        Observed::Rejected(Rejection::CursorMismatch),
        vec![itemized(read_rejected(0, Rejection::CursorMismatch), &[])],
    );
    push(
        GET_EXPIRED,
        Observed::Record(None),
        vec![get_miss_receipt()],
    );
    // book: ids 9, 13, ..., 37 and 41 (ids 1 and 5 are gone): 9 entries.
    push(
        EQ_AFTER,
        page_observed(model, &q_eq_book(), id_only, MAX_PAGE, None),
        vec![itemized(
            read_applied(46),
            &[
                (DirectoryLookup, 1),
                (QueryGroup, 1),
                (TermSeek, 1),
                (PostingEntry, 9),
                (ResultRow, 9),
            ],
        )],
    );
    // price:U64 holds the single value 5 (id 3). 1 + 5 + 4 + 3 + 1 + 3.
    push(
        RANGE_U64_AFTER,
        page_observed(model, &q_range_u64_price(), id_only, MAX_PAGE, None),
        vec![itemized(
            read_applied(17),
            &[
                (DirectoryLookup, 1),
                (QueryGroup, 1),
                (TermSeek, 1),
                (TermVisit, 1),
                (PostingEntry, 1),
                (ResultRow, 1),
            ],
        )],
    );
    // sofa: id 43 with its 6-byte key. 1 + 5 + 4 + 1 + 3 + 6.
    push(
        EQ_NEW_TERM_AFTER,
        page_observed(model, &q_eq_sofa(), key_only, MAX_PAGE, None),
        vec![itemized(
            read_applied(20),
            &[
                (DirectoryLookup, 1),
                (QueryGroup, 1),
                (TermSeek, 1),
                (PostingEntry, 1),
                (ResultRow, 1),
                (ReadByte, 6),
            ],
        )],
    );
}

/// Directory nodes on A's lookup path in phases 0 and 1. The directory is a
/// Merkle Patricia trie keyed by 8-byte big-endian ids (16 nibbles); A and B
/// (ids 1 and 2) share their first 15 nibbles, so the trie is an extension
/// over them, a branch on the last nibble and two leaves: 3 nodes.
const DIRECTORY_PATH_TWO: u64 = 3;

/// Directory nodes on A's lookup path in phase 2, with ids 1 to 502 (0x001
/// to 0x1f6): the ids share 13 zero nibbles (an extension), nibble 13 is 0
/// or 1 (a branch), ids 0x001 to 0x0ff span every value of nibble 14 (a
/// branch) and 0x001 to 0x00f every nonzero value of nibble 15 (a branch)
/// above A's leaf: 5 nodes.
const DIRECTORY_PATH_502: u64 = 5;

const _: () = assert!(DIRECTORY_PATH_TWO <= DIRECTORY_PATH_BOUND);
const _: () = assert!(DIRECTORY_PATH_502 <= DIRECTORY_PATH_BOUND);

/// Which of A's maps each kind of step touches, in every phase.
fn assert_physical_shape(steps: &[Step<Observed>], phase: &str) {
    let physical = |name| find(steps, name).physical();
    // Point reads walk the key map and the row map, never the index.
    for name in [GET_ALL, GET_NAMED, GET_ID_ONLY, GET_ABSENT, GET_FOREIGN] {
        assert_eq!(
            physical(name).index_nodes,
            0,
            "{phase}: {name} touches no index node"
        );
        assert!(
            physical(name).record_nodes > 0,
            "{phase}: {name} visits the key map"
        );
    }
    // The row map is the source of match-all and negated groups; probes load
    // rows, so the index is never touched.
    for name in [
        MATCH_ALL,
        NEGATED,
        COUNT_ALL,
        ABORT_MATCH_ALL,
        ABORT_NEGATED,
        ABORT_COUNT_ALL,
    ] {
        assert_eq!(
            physical(name).index_nodes,
            0,
            "{phase}: {name} touches no index node"
        );
        assert!(
            physical(name).record_nodes > 0,
            "{phase}: {name} visits the row map"
        );
    }
    // A miss seeks the term map and loads no row.
    for name in [
        MISS_EQ,
        MISS_FOREIGN_EQ,
        MISS_FOREIGN_PREFIX,
        MISS_FOREIGN_RANGE,
        MISS_NOT_EQ_TYPE,
    ] {
        assert!(
            physical(name).index_nodes > 0,
            "{phase}: {name} seeks the term map"
        );
        assert_eq!(
            physical(name).record_nodes,
            0,
            "{phase}: {name} loads no row"
        );
    }
    // A count over a posting with no probe never loads a row.
    assert_eq!(
        physical(COUNT_EQ).record_nodes,
        0,
        "{phase}: {COUNT_EQ} loads no row"
    );
    assert!(physical(COUNT_EQ).index_nodes > 0);
    // Admission rejections do no physical work at all.
    for name in [HOST_PUT, STALE_CURSOR] {
        assert_eq!(
            physical(name),
            PhysicalTrace::default(),
            "{phase}: {name} is refused before any work"
        );
    }
}

/// The engine with namespaces A (alice) and B (bob).
struct World {
    engine: Engine,
    a: NamespaceId,
    b: NamespaceId,
    /// The snapshot published at the end of phase 0.
    phase0: SnapshotId,
    /// B's terms so far, for the newness charge of later creates.
    b_terms: TermLedger,
}

impl World {
    /// Phase 0: one block creates A and B, fills A with 40 typed and 2 raw
    /// records and B with 5 typed records.
    fn publish_phase0() -> Self {
        let mut engine = Engine::new(PriceSchedule::FIXTURE);
        let mut branch = begin(&engine, None);
        let a = add_namespace(&mut branch, ALICE, "test");
        let b = add_namespace(&mut branch, BOB, "test");
        let mut expected = 2 * 53;
        let mut a_terms = TermLedger::default();
        for i in 0..A_RECORDS {
            let cells = a_cells(i);
            expected += create_cost(&a_key(i), &cells, a_terms.admit(&cells));
            let executed = apply(&mut branch, ALICE, create_record(a, a_key(i), cells));
            assert_eq!(
                executed.effect,
                Effect::RecordCreated {
                    id: rid(i as u64 + 1)
                }
            );
        }
        assert_eq!(a_terms.seen.len(), A_TERMS);
        for (key, value) in A_RAW_RECORDS {
            expected += put_cost(key, value);
            assert_eq!(
                apply(&mut branch, ALICE, put(a, key, value)).effect,
                Effect::Put { replaced: false }
            );
        }
        let mut b_terms = TermLedger::default();
        for j in 0..B_INITIAL_RECORDS {
            let cells = b_cells(j);
            expected += create_cost(&b_key(j), &cells, b_terms.admit(&cells));
            let executed = apply(&mut branch, BOB, create_record(b, b_key(j), cells));
            assert_eq!(
                executed.effect,
                Effect::RecordCreated {
                    id: rid(j as u64 + 1)
                }
            );
        }
        assert_eq!(
            expected, PHASE0_BLOCK_COST,
            "the transcribed table and the hand sum agree"
        );
        let phase0 = publish_block(&mut engine, branch, PHASE0_BLOCK_COST, "phase 0 block");
        let world = Self {
            engine,
            a,
            b,
            phase0,
            b_terms,
        };
        let head = world.head();
        assert_eq!(head.id(), phase0);
        assert_eq!(head.namespace_count(), PHASE0_NAMESPACES);
        assert_eq!(head.typed_record_count(a), Some(A_RECORDS));
        assert_eq!(head.term_count(a), Some(A_TERMS));
        assert_eq!(head.record_count(a), Some(A_RAW_RECORDS.len()));
        assert_eq!(head.typed_record_count(b), Some(B_INITIAL_RECORDS));
        assert_eq!(head.index_consistent(a), Some(true));
        assert_eq!(head.index_consistent(b), Some(true));
        world
    }

    /// Phase 1: 10 000 typed records into B across five blocks, then 2 000
    /// raw records in a sixth; no other namespace is touched, none created.
    fn grow_b(&mut self) {
        let per_block = B_GROWTH_RECORDS / B_GROWTH_BLOCKS;
        for (block, &block_cost) in B_GROWTH_BLOCK_COSTS.iter().enumerate() {
            let mut branch = begin(&self.engine, None);
            let first = B_INITIAL_RECORDS + block * per_block;
            let mut expected = 0;
            for j in first..first + per_block {
                let cells = b_cells(j);
                expected += create_cost(&b_key(j), &cells, self.b_terms.admit(&cells));
                let executed = apply(&mut branch, BOB, create_record(self.b, b_key(j), cells));
                assert_eq!(
                    executed.effect,
                    Effect::RecordCreated {
                        id: rid(j as u64 + 1)
                    },
                    "growth ids are dense"
                );
            }
            assert_eq!(
                expected, block_cost,
                "growth block {block}: table vs literal"
            );
            publish_block(
                &mut self.engine,
                branch,
                expected,
                &format!("growth block {block}"),
            );
        }
        let mut branch = begin(&self.engine, None);
        for r in 0..B_RAW_RECORDS {
            let executed = apply(
                &mut branch,
                BOB,
                put(self.b, &b_raw_key(r), &b_raw_value(r)),
            );
            assert_eq!(executed.effect, Effect::Put { replaced: false });
        }
        publish_block(&mut self.engine, branch, B_RAW_BLOCK_COST, "raw block");
        let head = self.head();
        assert_eq!(head.typed_record_count(self.b), Some(B_TOTAL_RECORDS));
        assert_eq!(head.record_count(self.b), Some(B_RAW_RECORDS));
        assert_eq!(head.index_consistent(self.b), Some(true));
    }

    /// Phase 2: one block creates 500 namespaces; no record is written.
    fn create_namespaces(&mut self) {
        let mut branch = begin(&self.engine, None);
        for _ in 0..EXTRA_NAMESPACES {
            add_namespace(&mut branch, DAVE, "test");
        }
        publish_block(&mut self.engine, branch, PHASE2_BLOCK_COST, "phase 2 block");
        assert_eq!(self.head().namespace_count(), PHASE2_NAMESPACES);
    }

    fn head(&self) -> Snapshot {
        self.engine.head_snapshot()
    }

    /// The precondition between phases: A's typed maps and raw map are
    /// physically the same objects and its counts and descriptor unchanged.
    fn assert_a_untouched_since(&self, before: &Snapshot, phase: &str) {
        let after = self.head();
        assert!(
            before.typed_shared_with(&after, self.a),
            "{phase}: A's rows, keys and index must be shared with the earlier head"
        );
        assert!(
            before.records_shared_with(&after, self.a),
            "{phase}: A's raw record map must be shared with the earlier head"
        );
        assert_eq!(
            after.typed_record_count(self.a),
            Some(A_RECORDS),
            "{phase}: A's typed size"
        );
        assert_eq!(
            after.term_count(self.a),
            Some(A_TERMS),
            "{phase}: A's term count"
        );
        assert_eq!(
            after.record_count(self.a),
            Some(A_RAW_RECORDS.len()),
            "{phase}: A's raw size"
        );
        assert_eq!(
            after.index_consistent(self.a),
            Some(true),
            "{phase}: A's index"
        );
        let described = after
            .descriptor(self.a, Budget::unlimited())
            .expect("A exists")
            .value;
        assert_eq!(
            described,
            descriptor(self.a, ALICE, 0),
            "{phase}: A's descriptor"
        );
    }
}

#[test]
fn baseline_matches_hand_derived_receipts_and_the_reference_model() {
    let world = World::publish_phase0();
    let before = world.head();
    let baseline = run_workload(&world);
    assert_matches_expected(&baseline, &expected_baseline(), "phase 0");
    assert_physical_shape(&baseline, "phase 0");
    assert_directory_nodes_follow_the_trie(&baseline, DIRECTORY_PATH_TWO, "phase 0");
    // The dropped branch changed nothing: not the rows, not the index, not
    // the raw map, not the owner.
    world.assert_a_untouched_since(&before, "phase 0 after the discarded branch");
    assert_eq!(world.head().id(), world.phase0);
    assert_eq!(world.engine.snapshot_count(), 2, "genesis and phase 0 only");
}

#[test]
fn workload_is_unchanged_by_typed_and_raw_growth_of_another_namespace() {
    let mut world = World::publish_phase0();
    let baseline = run_workload(&world);
    let before = world.head();

    world.grow_b();

    let after = world.head();
    assert_eq!(
        after.namespace_count(),
        PHASE0_NAMESPACES,
        "phase 1 creates no namespace"
    );
    assert!(
        !before.typed_shared_with(&after, world.b),
        "B's typed maps did change"
    );
    assert!(
        !before.records_shared_with(&after, world.b),
        "B's raw map did change"
    );
    world.assert_a_untouched_since(&before, "phase 1");

    let rerun = run_workload(&world);
    assert_matches_expected(&rerun, &expected_baseline(), "phase 1 against the literals");
    assert_receipts_and_results_equal(&baseline, &rerun, "phase 1");
    assert_namespace_local_nodes_equal(&baseline, &rerun, "phase 1");
    // Replacing B's state replaces the directory node's value only; the
    // directory keeps its shape, so A's lookup and update paths are the same.
    assert_directory_nodes_equal(&baseline, &rerun, "phase 1");
    assert_physical_shape(&rerun, "phase 1");
    assert_directory_nodes_follow_the_trie(&rerun, DIRECTORY_PATH_TWO, "phase 1");
    world.assert_a_untouched_since(&before, "phase 1 after the discarded branch");
}

#[test]
fn workload_is_unchanged_by_directory_growth_within_the_trie_bound() {
    let mut world = World::publish_phase0();
    let baseline = run_workload(&world);
    world.grow_b();
    let before = world.head();

    world.create_namespaces();

    let after = world.head();
    assert_eq!(
        after.typed_record_count(world.b),
        Some(B_TOTAL_RECORDS),
        "phase 2 writes nothing"
    );
    assert!(
        before.typed_shared_with(&after, world.b),
        "B's typed maps are untouched by namespace creation"
    );
    world.assert_a_untouched_since(&before, "phase 2");

    let rerun = run_workload(&world);
    assert_matches_expected(&rerun, &expected_baseline(), "phase 2 against the literals");
    assert_receipts_and_results_equal(&baseline, &rerun, "phase 2");
    assert_namespace_local_nodes_equal(&baseline, &rerun, "phase 2");
    // The directory did grow, so A's lookup path is longer: the shape for
    // 502 ids, within the 17-node bound of 8-byte keys.
    assert_directory_nodes_follow_the_trie(&rerun, DIRECTORY_PATH_502, "phase 2");
    assert_physical_shape(&rerun, "phase 2");
    world.assert_a_untouched_since(&before, "phase 2 after the discarded branch");
}

#[test]
fn pinned_phase0_snapshot_answers_identically_after_later_phases() {
    let mut world = World::publish_phase0();
    let baseline = run_workload(&world);
    world.grow_b();
    world.create_namespaces();

    let historical = world
        .engine
        .snapshot(world.phase0)
        .expect("phase 0 is retained");
    assert_eq!(historical.namespace_count(), PHASE0_NAMESPACES);
    assert_eq!(
        historical.typed_record_count(world.b),
        Some(B_INITIAL_RECORDS)
    );
    let reread = read_workload(&historical, world.a).steps;
    assert_eq!(reread.len(), READ_STEPS);
    // The pinned state is the very state the baseline branch was cloned
    // from, so even the physical layout is the same.
    assert_steps_repeat(&reread, &baseline, "historical");
    // The snapshot cursor of page 1 continues on the same snapshot only:
    // the head (a different snapshot of the same engine) refuses it.
    let traced = Budget::unlimited().traced();
    let page1 = historical
        .query(world.a, &q_eq_book(), &Projection::ALL, 4, None, traced)
        .expect("page 1");
    let cursor = page1.value.next.expect("more books follow");
    assert_eq!(cursor.snapshot(), Some(world.phase0));
    let refused = world
        .head()
        .query(
            world.a,
            &q_eq_book(),
            &Projection::ALL,
            4,
            Some(&cursor),
            traced,
        )
        .expect_err("a snapshot cursor binds its snapshot");
    assert_eq!(refused.reason, Rejection::CursorMismatch);
    assert_eq!(
        refused.receipt,
        itemized(read_rejected(0, Rejection::CursorMismatch), &[])
    );
    let continued = historical
        .query(
            world.a,
            &q_eq_book(),
            &Projection::ALL,
            4,
            Some(&cursor),
            traced,
        )
        .expect("page 2 on the pinned snapshot");
    assert_eq!(&continued.receipt, find(&baseline, EQ_PAGE2).receipt());
    assert!(historical.typed_shared_with(&world.head(), world.a));
}

#[test]
fn identical_terms_of_another_namespace_are_invisible_and_miss_at_fixed_cost() {
    let mut world = World::publish_phase0();
    world.grow_b();
    world.create_namespaces();
    let head = world.head();
    let traced = Budget::unlimited().traced();
    let (a, b) = (world.a, world.b);
    let expected = expected_baseline();
    let baseline_receipt = |name| expected_receipt(&expected, name);
    let ids_only = |namespace, query: &Query| {
        head.query(
            namespace,
            query,
            &Projection::ID_ONLY,
            MAX_PAGE,
            None,
            traced,
        )
        .expect("the namespace exists")
    };
    let model = phase0_model();

    // Everything A returns is one of its own 40 ids, while B holds 10 005
    // records under the same attribute names and values.
    let a_all = ids_only(a, &Query::match_all());
    let a_ids: Vec<RecordId> = a_all.value.rows.iter().map(|row| row.id).collect();
    assert_eq!(a_ids, model.ids(&Query::match_all()));
    assert!(a_ids.iter().all(|id| id.ordinal() <= A_RECORDS as u64));
    assert_eq!(a_all.receipt, baseline_receipt(MATCH_ALL));
    assert_eq!(head.typed_record_count(b), Some(B_TOTAL_RECORDS));

    // kind = book: 2502 records in B (j = 0, 4, ..., 10004), 10 in A at
    // the phase-0 price.
    let books_in_b = head.count(b, &q_eq_book(), traced).expect("B exists");
    assert_eq!(books_in_b.value, 2502);
    assert_eq!(
        books_in_b.receipt,
        itemized(
            read_applied(2512),
            &[
                (DirectoryLookup, 1),
                (QueryGroup, 1),
                (TermSeek, 1),
                (PostingEntry, 2502)
            ]
        )
    );
    let books_in_a = head.count(a, &q_eq_book(), traced).expect("A exists");
    assert_eq!(books_in_a.value, 10);
    assert_eq!(books_in_a.receipt, baseline_receipt(COUNT_EQ));

    // seq = 7 exists in B (id 8) and nowhere in A: A misses at the seek
    // price 10, B finds it at 1 + 5 + 4 + 1 + 3.
    let seq_in_b = ids_only(b, &q_foreign_eq());
    assert_eq!(
        seq_in_b.value.rows,
        vec![Row {
            id: rid(8),
            key: None,
            cells: Vec::new()
        }]
    );
    assert_eq!(seq_in_b.value.next, None);
    assert_eq!(
        seq_in_b.receipt,
        itemized(
            read_applied(14),
            &[
                (DirectoryLookup, 1),
                (QueryGroup, 1),
                (TermSeek, 1),
                (PostingEntry, 1),
                (ResultRow, 1)
            ]
        )
    );
    let seq_in_a = ids_only(a, &q_foreign_eq());
    assert!(seq_in_a.value.rows.is_empty());
    assert_eq!(seq_in_a.receipt, baseline_receipt(MISS_FOREIGN_EQ));

    // label starts with "v-00": a thousand distinct terms in B, each visited
    // and each holding one id: 1 + 5 + 4 + 3000 + 1000; an empty run in A.
    let labels_in_b = head
        .count(b, &q_foreign_prefix(), traced)
        .expect("B exists");
    assert_eq!(labels_in_b.value, 1000);
    assert_eq!(
        labels_in_b.receipt,
        itemized(
            read_applied(4010),
            &[
                (DirectoryLookup, 1),
                (QueryGroup, 1),
                (TermSeek, 1),
                (TermVisit, 1000),
                (PostingEntry, 1000)
            ]
        )
    );
    let labels_in_a = ids_only(a, &q_foreign_prefix());
    assert!(labels_in_a.value.rows.is_empty());
    assert_eq!(labels_in_a.receipt, baseline_receipt(MISS_FOREIGN_PREFIX));

    // A key of B through A misses at the fixed miss price; through B it is
    // the record with id 1.
    let through_a = head
        .get_record(a, &key(b_key(0)), &Projection::ID_ONLY, traced)
        .expect("A exists");
    assert_eq!(through_a.value, None);
    assert_eq!(through_a.receipt, baseline_receipt(GET_FOREIGN));
    let through_b = head
        .get_record(b, &key(b_key(0)), &Projection::ID_ONLY, traced)
        .expect("B exists");
    assert_eq!(
        through_b.value,
        Some(Row {
            id: rid(1),
            key: None,
            cells: Vec::new()
        })
    );
    assert_eq!(
        through_b.receipt,
        itemized(
            read_applied(7),
            &[(DirectoryLookup, 1), (PointRead, 1), (ResultRow, 1)]
        )
    );
    // The other direction: A's key through B.
    let a_key_through_b = head
        .get_record(b, &key(a_key(7)), &Projection::ALL, traced)
        .expect("B exists");
    assert_eq!(a_key_through_b.value, None);
    assert_eq!(a_key_through_b.receipt, baseline_receipt(GET_ABSENT));
}

#[test]
fn raw_keyspaces_are_separate_and_see_nothing_of_the_typed_records() {
    let mut world = World::publish_phase0();
    world.grow_b();
    let head = world.head();
    let traced = Budget::unlimited().traced();
    let (a, b) = (world.a, world.b);

    // A's scan returns its two raw records at 5 + 2 * (2 + 5 + 3), B's `r/`
    // scan its 2000 at 5 + 2000 * (2 + 7 + 9).
    let a_raw = head.scan_prefix(a, b"", traced).expect("A exists");
    assert_eq!(
        a_raw
            .value
            .iter()
            .map(|entry| entry.key.as_slice())
            .collect::<Vec<_>>(),
        vec![b"raw/1".as_slice(), b"raw/2"]
    );
    assert_eq!(
        a_raw.receipt,
        itemized(
            read_applied(25),
            &[
                (DirectoryLookup, 1),
                (ScanSeek, 1),
                (ScanEntry, 2),
                (ReadByte, 16)
            ]
        )
    );
    let b_raw = head.scan_prefix(b, b"r/", traced).expect("B exists");
    assert_eq!(b_raw.value.len(), B_RAW_RECORDS);
    assert_eq!(
        b_raw.receipt,
        itemized(
            read_applied(36_005),
            &[
                (DirectoryLookup, 1),
                (ScanSeek, 1),
                (ScanEntry, 2000),
                (ReadByte, 32_000)
            ]
        )
    );
    let b_raw_through_a = head.scan_prefix(a, b"r/", traced).expect("A exists");
    assert!(b_raw_through_a.value.is_empty());
    assert_eq!(
        b_raw_through_a.receipt,
        itemized(read_applied(5), &[(DirectoryLookup, 1), (ScanSeek, 1)])
    );
}

#[test]
fn expiry_sweeps_only_the_named_namespace() {
    let mut world = World::publish_phase0();
    world.grow_b();
    let before = world.head();
    let (a, b) = (world.a, world.b);
    let traced = Budget::unlimited().traced();
    let unlimited = Budget::unlimited();
    let expiry = name("$exp");
    // Both namespaces hold $exp 1, 2, 3 on their ids 1, 6, 11: the run
    // visits three terms with one entry each. 1 + 5 + 4 + 9 + 3.
    let three = itemized(
        read_applied(22),
        &[
            (DirectoryLookup, 1),
            (QueryGroup, 1),
            (TermSeek, 1),
            (TermVisit, 3),
            (PostingEntry, 3),
        ],
    );
    let none = itemized(
        read_applied(10),
        &[(DirectoryLookup, 1), (QueryGroup, 1), (TermSeek, 1)],
    );
    for namespace in [a, b] {
        let count = before
            .count(namespace, &q_expiry(), traced)
            .expect("exists");
        assert_eq!(count.value, 3);
        assert_eq!(count.receipt, three);
    }

    let mut branch = begin(&world.engine, None);
    // Sweeping A deletes a/00 (130), a/05 and a/10 (106 each) and nothing of
    // B.
    let swept_a = expire_records(&mut branch, a, &expiry, EXPIRY_BLOCK, unlimited, unlimited)
        .expect("A's sweep completes");
    assert_eq!(
        swept_a,
        vec![
            (rid(1), applied(130)),
            (rid(6), applied(106)),
            (rid(11), applied(106)),
        ]
    );
    assert_eq!(branch.typed_record_count(a), Some(A_RECORDS - 3));
    assert_eq!(
        branch.typed_record_count(b),
        Some(B_TOTAL_RECORDS),
        "B is untouched"
    );
    assert_eq!(
        branch
            .count(a, &q_expiry(), traced)
            .expect("A exists")
            .receipt,
        none
    );
    let b_after_a = branch.count(b, &q_expiry(), traced).expect("B exists");
    assert_eq!(b_after_a.value, 3, "B's expiring records are still live");
    assert_eq!(b_after_a.receipt, three);
    assert_eq!(branch.spent(), Cost::new(130 + 106 + 106));

    // Sweeping B deletes its own three: b/00000 has 7 cells and 6 attributes
    // (4 + (20 + 28 + 6) + 12 * 10 = 178), b/00005 and b/00010 6 cells and 5
    // attributes (4 + (20 + 24 + 6) + 10 * 10 = 154).
    let swept_b = expire_records(&mut branch, b, &expiry, EXPIRY_BLOCK, unlimited, unlimited)
        .expect("B's sweep completes");
    assert_eq!(
        swept_b,
        vec![
            (rid(1), applied(178)),
            (rid(6), applied(154)),
            (rid(11), applied(154)),
        ]
    );
    assert_eq!(branch.typed_record_count(b), Some(B_TOTAL_RECORDS - 3));
    assert_eq!(
        branch.typed_record_count(a),
        Some(A_RECORDS - 3),
        "A is untouched"
    );
    assert_eq!(
        branch
            .count(b, &q_expiry(), traced)
            .expect("B exists")
            .receipt,
        none
    );
    assert_eq!(branch.index_consistent(a), Some(true));
    assert_eq!(branch.index_consistent(b), Some(true));
    assert_eq!(branch.receipts().len(), 6);

    // The branch is dropped: both published namespaces are exactly as before.
    drop(branch);
    let after = world.head();
    assert_eq!(after.id(), before.id());
    assert!(before.typed_shared_with(&after, a));
    assert!(before.typed_shared_with(&after, b));
    assert_eq!(after.typed_record_count(a), Some(A_RECORDS));
    assert_eq!(after.typed_record_count(b), Some(B_TOTAL_RECORDS));
}
