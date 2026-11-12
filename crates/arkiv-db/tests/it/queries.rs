//! Query operator semantics against an independent full-scan reference, the
//! validation order of every `QueryFault`, and the hand-derived charges of
//! cells of another type and absent cells on a second, six-record fixture.
//!
//! The semantics fixture is one namespace of 45 records whose cell names
//! overlap on purpose: `n`, `m` and `k` are attributes of different types in
//! some records, fields holding the same values in others and absent in the
//! rest; `q` never occurs. Expected pages and counts come from a `BTreeMap`
//! model beside the engine, never from an engine index, and every check runs
//! as written and planned. Write charges are in `query_metering.rs`, the
//! cursor-binding matrix in `pagination.rs`.

use std::{fmt::Debug, ops::Bound};

use arkiv_db::{
    Branch, Budget, Cell, CellName, Effect, Engine, Limits, Literal, NamespaceId, OperationClass,
    PriceSchedule, Projection, Query, QueryFault, ReadResult, Receipt, RecordId, Rejection, Row,
    Snapshot, Term, TypeTag, Value,
};

use crate::support::{
    model::Model,
    random::Generator64,
    reader::Reader,
    vocab::{
        self, ALICE, add_namespace, apply, attr, begin, create_record, delete_record, field, key,
        ledger, name, names, ns, patch_record, publish, read_applied, read_rejected, rid, traced,
    },
    walk::{rows_of, walk},
};

fn s(text: &str) -> Value {
    Value::str(text)
}

fn named(cells: Vec<(&str, Cell)>) -> Vec<(CellName, Cell)> {
    cells
        .into_iter()
        .map(|(text, cell)| (name(text), cell))
        .collect()
}

fn ordinals(rows: &[Row]) -> Vec<u64> {
    rows.iter().map(|row| row.id.ordinal()).collect()
}

/// Created in this order, so `rNN` has id NN.
fn fixture_records() -> Vec<(&'static str, Vec<(&'static str, Cell)>)> {
    let (min, max, umax) = (i64::MIN, i64::MAX, u64::MAX);
    vec![
        // Str attributes of `n`; `m` cycles through every type.
        ("r01", vec![("n", attr(s(""))), ("m", attr(Value::I64(0)))]),
        ("r02", vec![("n", attr(s("a"))), ("m", attr(Value::I64(1)))]),
        (
            "r03",
            vec![("n", attr(s("ab"))), ("m", attr(Value::I64(-1)))],
        ),
        (
            "r04",
            vec![("n", attr(s("abc"))), ("m", attr(Value::I64(min)))],
        ),
        (
            "r05",
            vec![("n", attr(s("b"))), ("m", attr(Value::I64(max)))],
        ),
        ("r06", vec![("n", attr(s("ä"))), ("m", attr(Value::U64(0)))]),
        (
            "r07",
            vec![("n", attr(s("äb"))), ("m", attr(Value::U64(umax)))],
        ),
        ("r08", vec![("n", attr(s("aä"))), ("m", attr(s("a")))]),
        (
            "r09",
            vec![("n", attr(s("abc"))), ("m", attr(Value::Bool(true)))],
        ),
        ("r10", vec![("n", attr(s("a")))]),
        // I64 attributes of `n`.
        (
            "r11",
            vec![("n", attr(Value::I64(min))), ("m", attr(s("ab")))],
        ),
        (
            "r12",
            vec![("n", attr(Value::I64(-1))), ("m", attr(s("abc")))],
        ),
        ("r13", vec![("n", attr(Value::I64(0))), ("m", attr(s("")))]),
        (
            "r14",
            vec![("n", attr(Value::I64(1))), ("m", attr(Value::I64(0)))],
        ),
        (
            "r15",
            vec![("n", attr(Value::I64(7))), ("m", attr(Value::I64(7)))],
        ),
        (
            "r16",
            vec![("n", attr(Value::I64(max))), ("m", attr(Value::U64(7)))],
        ),
        (
            "r17",
            vec![("n", attr(Value::I64(0))), ("m", field(Value::I64(0)))],
        ),
        ("r18", vec![("n", attr(Value::I64(7)))]),
        // U64 attributes of `n`.
        (
            "r19",
            vec![("n", attr(Value::U64(0))), ("m", attr(Value::Bool(false)))],
        ),
        (
            "r20",
            vec![("n", attr(Value::U64(1))), ("m", attr(Value::I64(1)))],
        ),
        (
            "r21",
            vec![("n", attr(Value::U64(7))), ("m", attr(Value::U64(1)))],
        ),
        (
            "r22",
            vec![("n", attr(Value::U64(umax))), ("m", attr(Value::I64(-1)))],
        ),
        ("r23", vec![("n", attr(Value::U64(7)))]),
        // Bool attributes of `n`.
        (
            "r24",
            vec![("n", attr(Value::Bool(true))), ("m", attr(s("b")))],
        ),
        (
            "r25",
            vec![("n", attr(Value::Bool(false))), ("m", attr(s("a")))],
        ),
        (
            "r26",
            vec![("n", attr(Value::Bool(true))), ("m", attr(Value::I64(7)))],
        ),
        // Fields of `n` holding values that attributes of `n` also hold.
        (
            "r27",
            vec![("n", field(s("a"))), ("m", attr(Value::I64(0)))],
        ),
        (
            "r28",
            vec![("n", field(Value::I64(0))), ("m", attr(s("a")))],
        ),
        (
            "r29",
            vec![("n", field(Value::U64(7))), ("m", attr(Value::U64(7)))],
        ),
        (
            "r30",
            vec![
                ("n", field(Value::Bool(true))),
                ("m", attr(Value::Bool(true))),
            ],
        ),
        (
            "r31",
            vec![("n", field(Value::bytes(b"a"))), ("m", attr(Value::I64(1)))],
        ),
        // Records without `n`.
        ("r32", vec![("m", attr(s("abc")))]),
        ("r33", vec![("m", attr(Value::I64(min)))]),
        ("r34", vec![("m", field(s("abc")))]),
        ("r35", vec![("k", attr(s("only")))]),
        // Three-cell records and more duplicates.
        (
            "r36",
            vec![
                ("n", attr(s("abc"))),
                ("m", attr(Value::I64(7))),
                ("k", attr(Value::Bool(true))),
            ],
        ),
        (
            "r37",
            vec![
                ("n", attr(Value::I64(7))),
                ("m", attr(s("abc"))),
                ("k", attr(Value::U64(umax))),
            ],
        ),
        (
            "r38",
            vec![("n", attr(Value::U64(1))), ("m", attr(Value::U64(umax)))],
        ),
        (
            "r39",
            vec![
                ("n", attr(s("a"))),
                ("m", attr(s("a"))),
                ("k", field(Value::bytes(b""))),
            ],
        ),
        (
            "r40",
            vec![
                ("n", attr(Value::Bool(false))),
                ("m", attr(Value::Bool(false))),
            ],
        ),
        (
            "r41",
            vec![("n", attr(s("ab"))), ("m", attr(Value::I64(1)))],
        ),
        (
            "r42",
            vec![
                ("n", attr(Value::I64(1))),
                ("m", attr(Value::I64(-1))),
                ("k", attr(Value::I64(-1))),
            ],
        ),
        (
            "r43",
            vec![("n", attr(Value::U64(0))), ("m", attr(Value::U64(0)))],
        ),
        (
            "r44",
            vec![("n", attr(s("\u{10FFFF}"))), ("m", attr(Value::I64(max)))],
        ),
        (
            "r45",
            vec![("n", attr(s("aä"))), ("m", attr(Value::I64(7)))],
        ),
    ]
}

const FIXTURE_SIZE: usize = 45;

/// A branch and the model kept in step with it; every mutation goes through
/// both and checks the index invariant and the unmetered counts afterwards.
struct World {
    ns: NamespaceId,
    branch: Branch,
    model: Model,
}

impl World {
    fn begin(engine: &Engine, ns: NamespaceId, model: Model) -> Self {
        Self {
            ns,
            branch: begin(engine, None),
            model,
        }
    }

    fn create(&mut self, key_text: &str, cells: Vec<(&str, Cell)>) -> RecordId {
        let cells = named(cells);
        let expected = self.model.create(key_text, &cells);
        let op = create_record(self.ns, key_text, cells);
        let executed = apply(&mut self.branch, ALICE, op);
        assert_eq!(executed.effect, Effect::RecordCreated { id: expected });
        self.check_invariants();
        expected
    }

    fn patch(&mut self, key_text: &str, set: Vec<(&str, Cell)>, remove: &[&str]) -> RecordId {
        let set = named(set);
        let remove = names(remove);
        let expected = self.model.patch(key_text, &set, &remove);
        let op = patch_record(self.ns, key_text, set, remove);
        let executed = apply(&mut self.branch, ALICE, op);
        assert_eq!(executed.effect, Effect::RecordPatched { id: expected });
        self.check_invariants();
        expected
    }

    fn delete(&mut self, key_text: &str) -> RecordId {
        let expected = self.model.delete(key_text);
        let executed = apply(&mut self.branch, ALICE, delete_record(self.ns, key_text));
        assert_eq!(executed.effect, Effect::RecordDeleted { id: expected });
        self.check_invariants();
        expected
    }

    fn check_invariants(&self) {
        assert_eq!(self.branch.index_consistent(self.ns), Some(true));
        assert_eq!(
            self.branch.typed_record_count(self.ns),
            Some(self.model.len())
        );
        assert_eq!(
            self.branch.term_count(self.ns),
            Some(self.model.term_count())
        );
    }
}

/// Snapshot 1 holds namespace 1 (owned by ALICE) with the 45 fixture
/// records; the returned model mirrors it.
fn semantics_fixture() -> (Engine, NamespaceId, Model) {
    let mut engine = Engine::new(PriceSchedule::FIXTURE);
    let ns = ns(1);
    let mut world = World::begin(&engine, ns, Model::new());
    assert_eq!(add_namespace(&mut world.branch, ALICE, "test"), ns);
    for (key_text, cells) in fixture_records() {
        world.create(key_text, cells);
    }
    assert_eq!(world.model.len(), FIXTURE_SIZE);
    assert_eq!(publish(&mut engine, world.branch).sequence(), 1);
    (engine, ns, world.model)
}

// Query generation: every single-literal term over the value universe, both
// polarities, plus deterministic AND / OR combinations.

/// `q` never occurs in any record.
const NAMES: [&str; 4] = ["n", "m", "k", "q"];
/// Present and absent text values: the empty string, a chain of prefixes,
/// non-ASCII text, the largest code point, a case variant, and an absent one.
const STRS: [&str; 12] = [
    "",
    "a",
    "ab",
    "abc",
    "abcd",
    "b",
    "ä",
    "äb",
    "aä",
    "\u{10FFFF}",
    "A",
    "zzz",
];
const PREFIXES: [&str; 12] = [
    "",
    "a",
    "ab",
    "abc",
    "abcd",
    "b",
    "ä",
    "äb",
    "aä",
    "Ä",
    "z",
    "\u{10FFFF}",
];
const I64S: [i64; 9] = [i64::MIN, i64::MIN + 1, -2, -1, 0, 1, 2, 7, i64::MAX];
const U64S: [u64; 6] = [0, 1, 2, 7, u64::MAX - 1, u64::MAX];
const INDEXABLE: [TypeTag; 4] = [TypeTag::Bool, TypeTag::I64, TypeTag::U64, TypeTag::Str];

fn universe_values() -> Vec<Value> {
    let mut values: Vec<Value> = STRS.iter().map(|text| s(text)).collect();
    values.extend(I64S.iter().map(|&value| Value::I64(value)));
    values.extend(U64S.iter().map(|&value| Value::U64(value)));
    values.extend([Value::Bool(true), Value::Bool(false)]);
    values
}

/// Every (Included, Excluded) combination over every ordered pair of the
/// bound values, including `lower == upper` and `lower > upper`, plus every
/// one-sided range.
fn all_bound_pairs(values: &[Value]) -> Vec<(Bound<Value>, Bound<Value>)> {
    let mut pairs = Vec::new();
    for low in values {
        for high in values {
            pairs.push((Bound::Included(low.clone()), Bound::Included(high.clone())));
            pairs.push((Bound::Included(low.clone()), Bound::Excluded(high.clone())));
            pairs.push((Bound::Excluded(low.clone()), Bound::Included(high.clone())));
            pairs.push((Bound::Excluded(low.clone()), Bound::Excluded(high.clone())));
        }
        pairs.push((Bound::Unbounded, Bound::Included(low.clone())));
        pairs.push((Bound::Unbounded, Bound::Excluded(low.clone())));
        pairs.push((Bound::Included(low.clone()), Bound::Unbounded));
        pairs.push((Bound::Excluded(low.clone()), Bound::Unbounded));
    }
    pairs
}

/// A short, fixed set of ranges for the per-mutation checks.
fn few_bound_pairs() -> Vec<(Bound<Value>, Bound<Value>)> {
    vec![
        (Bound::Unbounded, Bound::Included(Value::I64(0))),
        (
            Bound::Excluded(Value::I64(-1)),
            Bound::Included(Value::I64(7)),
        ),
        (
            Bound::Included(Value::I64(0)),
            Bound::Excluded(Value::I64(0)),
        ),
        (
            Bound::Included(Value::I64(i64::MIN)),
            Bound::Included(Value::I64(i64::MAX)),
        ),
        (Bound::Excluded(Value::I64(7)), Bound::Unbounded),
        (
            Bound::Included(Value::I64(7)),
            Bound::Included(Value::I64(1)),
        ),
        (Bound::Unbounded, Bound::Excluded(Value::U64(u64::MAX))),
        (Bound::Included(Value::U64(u64::MAX)), Bound::Unbounded),
        (
            Bound::Excluded(Value::U64(0)),
            Bound::Included(Value::U64(7)),
        ),
        (
            Bound::Included(Value::U64(7)),
            Bound::Included(Value::U64(7)),
        ),
    ]
}

fn single_terms(full: bool) -> Vec<Term> {
    let values = universe_values();
    let i64_bounds: Vec<Value> = I64S.iter().map(|&value| Value::I64(value)).collect();
    let u64_bounds: Vec<Value> = U64S.iter().map(|&value| Value::U64(value)).collect();
    let mut terms = Vec::new();
    for text in NAMES {
        let name = name(text);
        for value in &values {
            terms.push(Term::eq(name.clone(), value.clone()));
            terms.push(Term::not_eq(name.clone(), value.clone()));
        }
        for tag in INDEXABLE {
            terms.push(Term::exists(name.clone(), tag));
        }
        for prefix in PREFIXES {
            terms.push(Term::starts_with(name.clone(), prefix));
        }
        let pairs = if full {
            let mut pairs = all_bound_pairs(&i64_bounds);
            pairs.extend(all_bound_pairs(&u64_bounds));
            pairs
        } else {
            few_bound_pairs()
        };
        for (lower, upper) in pairs {
            terms.push(Term::range(name.clone(), lower, upper));
        }
    }
    terms
}

/// Every term as a positive and as a negated literal.
fn literal_pool(full: bool) -> Vec<Literal> {
    single_terms(full)
        .into_iter()
        .flat_map(|term| [Literal::of(term.clone()), Literal::not(term)])
        .collect()
}

fn single_literal_queries(full: bool) -> Vec<Query> {
    literal_pool(full)
        .into_iter()
        .map(|literal| Query::all(vec![literal]))
        .collect()
}

/// `size` literals drawn from `pool`.
fn group(generator: &mut Generator64, pool: &[Literal], size: usize) -> Vec<Literal> {
    (0..size)
        .map(|_| pool[generator.below(pool.len())].clone())
        .collect()
}

/// The seed is fixed: the combination set is a pure function of the pool
/// order.
fn combined_queries(pool: &[Literal]) -> Vec<Query> {
    let mut generator = Generator64(0x2545_f491_4f6c_dd1d);
    let mut queries = Vec::new();
    for _ in 0..300 {
        queries.push(Query::all(group(&mut generator, pool, 2)));
    }
    for _ in 0..200 {
        queries.push(Query::all(group(&mut generator, pool, 3)));
    }
    for _ in 0..200 {
        let first_size = 1 + generator.below(2);
        let first = group(&mut generator, pool, first_size);
        let second_size = 1 + generator.below(2);
        let second = group(&mut generator, pool, second_size);
        queries.push(Query {
            groups: vec![first, second],
        });
    }
    for round in 0..100 {
        let first = group(&mut generator, pool, 2);
        let middle = if round % 5 == 0 {
            Vec::new()
        } else {
            group(&mut generator, pool, 1)
        };
        let last_size = 1 + generator.below(3);
        let last = group(&mut generator, pool, last_size);
        queries.push(Query {
            groups: vec![first, middle, last],
        });
    }
    for _ in 0..50 {
        let group = group(&mut generator, pool, 2);
        queries.push(Query {
            groups: vec![group.clone(), group],
        });
    }
    // Contradictions, tautologies and the empty group in every position.
    let samples = [
        (s("a"), TypeTag::Str),
        (Value::I64(7), TypeTag::I64),
        (Value::U64(7), TypeTag::U64),
        (Value::Bool(true), TypeTag::Bool),
    ];
    for text in NAMES {
        let name = name(text);
        for (value, tag) in &samples {
            let eq = Term::eq(name.clone(), value.clone());
            let not_eq = Term::not_eq(name.clone(), value.clone());
            let exists = Term::exists(name.clone(), *tag);
            queries.push(Query::all(vec![
                Literal::of(eq.clone()),
                Literal::of(not_eq),
            ]));
            queries.push(Query::all(vec![
                Literal::of(eq.clone()),
                Literal::not(eq.clone()),
            ]));
            queries.push(Query {
                groups: vec![
                    vec![Literal::of(eq.clone())],
                    vec![Literal::not(eq.clone())],
                ],
            });
            queries.push(Query {
                groups: vec![
                    vec![Literal::of(exists.clone())],
                    vec![Literal::not(exists.clone())],
                ],
            });
            queries.push(Query {
                groups: vec![Vec::new(), vec![Literal::of(eq.clone())]],
            });
            queries.push(Query {
                groups: vec![vec![Literal::of(eq)], Vec::new()],
            });
        }
    }
    queries.push(Query::match_all());
    queries
}

/// Both evaluations: as written (the canonical charges) and planned.
const EVALUATIONS: [Budget; 2] = [Budget::unlimited(), Budget::unlimited().planned()];

/// Rows and count of `query` on the engine equal the model's full scan,
/// under both evaluations.
fn check_query<R: Reader>(
    reader: &R,
    model: &Model,
    ns: NamespaceId,
    query: &Query,
    projection: &Projection,
    limits: &[usize],
) {
    let expected_ids = model.ids(query);
    let expected_rows = model.evaluate(query, projection);
    for budget in EVALUATIONS {
        let planned = budget.is_planned();
        let limit_of = |page: usize| limits[page % limits.len()];
        let rows = rows_of(&walk(
            reader,
            ns,
            query,
            projection,
            budget,
            limit_of,
            |_| {},
        ));
        assert_eq!(
            ordinals(&rows),
            expected_ids
                .iter()
                .map(|id| id.ordinal())
                .collect::<Vec<_>>(),
            "{query:?} with limits {limits:?} (planned: {planned})"
        );
        assert_eq!(
            rows, expected_rows,
            "{query:?}: projected rows (planned: {planned})"
        );
        let count = reader
            .count(ns, query, budget)
            .unwrap_or_else(|rejected| panic!("count {query:?}: {rejected}"))
            .value;
        assert_eq!(
            count,
            u64::try_from(expected_ids.len()).expect("counts fit u64"),
            "count {query:?} (planned: {planned})"
        );
    }
}

const LIMIT_PATTERNS: [&[usize]; 4] = [&[1], &[2, 3], &[5, 1, 200], &[200]];

fn check_queries<R: Reader>(reader: &R, model: &Model, ns: NamespaceId, queries: &[Query]) {
    for (index, query) in queries.iter().enumerate() {
        let limits = LIMIT_PATTERNS[index % LIMIT_PATTERNS.len()];
        check_query(reader, model, ns, query, &Projection::ALL, limits);
    }
}

/// Ids of every match on one page of 200 (the fixture never exceeds it).
fn ids_of<R: Reader>(reader: &R, ns: NamespaceId, query: &Query) -> Vec<u64> {
    let page = reader
        .query(
            ns,
            query,
            &Projection::ID_ONLY,
            200,
            None,
            Budget::unlimited(),
        )
        .unwrap_or_else(|rejected| panic!("{query:?}: {rejected}"))
        .value;
    assert!(page.next.is_none(), "{query:?}: more than 200 matches");
    ordinals(&page.rows)
}

fn eq(text: &str, value: Value) -> Query {
    Query::term(Term::eq(name(text), value))
}

#[test]
fn every_single_literal_matches_the_full_scan_reference() {
    let (engine, ns, model) = semantics_fixture();
    let snapshot = engine.head_snapshot();
    let queries = single_literal_queries(true);
    assert!(queries.len() > 4_000, "the generated set is large");
    check_queries(&snapshot, &model, ns, &queries);
}

#[test]
fn and_groups_and_or_of_groups_match_the_full_scan_reference() {
    let (engine, ns, model) = semantics_fixture();
    let snapshot = engine.head_snapshot();
    let queries = combined_queries(&literal_pool(true));
    check_queries(&snapshot, &model, ns, &queries);
}

#[test]
fn a_branch_answers_like_the_snapshot_it_was_begun_from() {
    let (engine, ns, model) = semantics_fixture();
    let branch = begin(&engine, None);
    check_queries(&branch, &model, ns, &single_literal_queries(false));
    check_queries(&branch, &model, ns, &combined_queries(&literal_pool(false)));
}

#[test]
fn hand_picked_operator_cases_hold() {
    let (engine, ns, _) = semantics_fixture();
    let snapshot = engine.head_snapshot();
    let ids = |query: Query| ids_of(&snapshot, ns, &query);
    let n = || name("n");

    // Eq on every type of the same name; fields (r27..r31) never match.
    assert_eq!(ids(eq("n", s("a"))), [2, 10, 39]);
    assert_eq!(ids(eq("n", Value::I64(0))), [13, 17]);
    assert_eq!(ids(eq("n", Value::U64(7))), [21, 23]);
    assert_eq!(ids(eq("n", Value::Bool(true))), [24, 26]);
    assert_eq!(ids(eq("n", s(""))), [1]);
    assert_eq!(ids(eq("n", Value::I64(i64::MIN))), [11]);
    assert_eq!(ids(eq("n", Value::U64(u64::MAX))), [22]);
    assert_eq!(ids(eq("n", s("\u{10FFFF}"))), [44]);
    assert_eq!(ids(eq("n", s("A"))), [] as [u64; 0]);
    assert_eq!(ids(eq("q", s("a"))), [] as [u64; 0]);
    // Same numeric value, other type: absent.
    assert_eq!(ids(eq("n", Value::I64(7))), [15, 18, 37]);
    assert_eq!(ids(eq("n", Value::U64(0))), [19, 43]);

    // NotEq needs an attribute of the value's type; other types, fields and
    // records lacking the name do not match.
    assert_eq!(
        ids(Query::term(Term::not_eq(n(), Value::Bool(true)))),
        [25, 40]
    );
    assert_eq!(
        ids(Query::term(Term::not_eq(n(), Value::U64(7)))),
        [19, 20, 22, 38, 43]
    );
    // Its negation is the complement: everything else, fields included.
    assert_eq!(
        ids(Query::all(vec![Literal::not(Term::not_eq(
            n(),
            Value::U64(7)
        ))]))
        .len(),
        FIXTURE_SIZE - 5
    );

    // Range: every bound combination on the same endpoints; lower > upper
    // is empty; extremes are ordinary values.
    let range =
        |lower: Bound<Value>, upper: Bound<Value>| ids(Query::term(Term::range(n(), lower, upper)));
    let (inc, exc) = (
        |value: i64| Bound::Included(Value::I64(value)),
        |value: i64| Bound::Excluded(Value::I64(value)),
    );
    assert_eq!(range(inc(-1), inc(1)), [12, 13, 14, 17, 42]);
    assert_eq!(range(exc(-1), inc(1)), [13, 14, 17, 42]);
    assert_eq!(range(inc(-1), exc(1)), [12, 13, 17]);
    assert_eq!(range(exc(-1), exc(1)), [13, 17]);
    assert_eq!(range(inc(1), inc(-1)), [] as [u64; 0]);
    assert_eq!(range(exc(0), exc(0)), [] as [u64; 0]);
    assert_eq!(range(inc(0), inc(0)), [13, 17]);
    assert_eq!(range(Bound::Unbounded, exc(0)), [11, 12]);
    assert_eq!(range(inc(i64::MAX), Bound::Unbounded), [16]);
    assert_eq!(range(Bound::Unbounded, inc(i64::MIN)), [11]);
    assert_eq!(
        range(
            Bound::Excluded(Value::U64(0)),
            Bound::Included(Value::U64(u64::MAX))
        ),
        [20, 21, 22, 23, 38]
    );
    assert_eq!(
        range(Bound::Unbounded, Bound::Excluded(Value::U64(u64::MAX))),
        [19, 20, 21, 23, 38, 43]
    );

    // StartsWith: empty prefix is every Str attribute; a prefix that is also
    // a value; non-ASCII prefixes; no other type matches.
    let prefix = |prefix: &str| ids(Query::term(Term::starts_with(n(), prefix)));
    assert_eq!(
        prefix(""),
        [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 36, 39, 41, 44, 45]
    );
    assert_eq!(prefix("a"), [2, 3, 4, 8, 9, 10, 36, 39, 41, 45]);
    assert_eq!(prefix("ab"), [3, 4, 9, 36, 41]);
    assert_eq!(prefix("abc"), [4, 9, 36]);
    assert_eq!(prefix("abcd"), [] as [u64; 0]);
    assert_eq!(prefix("ä"), [6, 7]);
    assert_eq!(prefix("aä"), [8, 45]);
    assert_eq!(prefix("Ä"), [] as [u64; 0]);

    // Exists per type; the Bytes field of r31 is nothing to any query.
    let exists = |tag: TypeTag| ids(Query::term(Term::exists(n(), tag)));
    assert_eq!(exists(TypeTag::Str).len(), 15);
    assert_eq!(
        exists(TypeTag::I64),
        [11, 12, 13, 14, 15, 16, 17, 18, 37, 42]
    );
    assert_eq!(exists(TypeTag::U64), [19, 20, 21, 22, 23, 38, 43]);
    assert_eq!(exists(TypeTag::Bool), [24, 25, 26, 40]);
    assert_eq!(
        ids_of(&snapshot, ns, &Query::match_all()).len(),
        FIXTURE_SIZE
    );
}

#[test]
fn projections_return_only_what_they_name() {
    let (engine, ns, model) = semantics_fixture();
    let snapshot = engine.head_snapshot();
    let queries = [
        eq("n", s("a")),
        Query::term(Term::exists(name("k"), TypeTag::Str)),
        Query::term(Term::exists(name("m"), TypeTag::I64)),
        Query::match_all(),
    ];
    let projections = [
        Projection::ID_ONLY,
        Projection::KEY_ONLY,
        Projection::named(false, [name("n"), name("q")]),
        Projection::named(true, [name("k")]),
        Projection::named(true, []),
    ];
    for query in &queries {
        for projection in &projections {
            check_query(&snapshot, &model, ns, query, projection, &[3, 200]);
        }
    }
    // A named projection skips names the record lacks.
    let page = snapshot
        .query(
            ns,
            &eq("k", s("only")),
            &Projection::named(true, [name("n"), name("q")]),
            5,
            None,
            Budget::unlimited(),
        )
        .expect("a valid query")
        .value;
    assert_eq!(
        page.rows,
        [Row {
            id: rid(35),
            key: Some(key("r35")),
            cells: Vec::new(),
        }]
    );
    let row = snapshot
        .get_record(
            ns,
            &key("r35"),
            &Projection::named(false, [name("k"), name("n")]),
            Budget::unlimited(),
        )
        .expect("a valid read")
        .value
        .expect("r35 is live");
    assert_eq!(
        row,
        Row {
            id: rid(35),
            key: None,
            cells: vec![(name("k"), attr(s("only")))],
        }
    );
}

#[test]
fn patches_and_deletes_move_records_between_indexes() {
    let (mut engine, ns, model) = semantics_fixture();
    let original = model.clone();
    let fixture_snapshot = engine.head();
    let mut world = World::begin(&engine, ns, model);
    let probes = single_literal_queries(false);
    let n = || name("n");
    let m = || name("m");

    // Type change: I64 7 -> U64 7 leaves one posting and joins another.
    assert_eq!(
        ids_of(&world.branch, ns, &eq("n", Value::I64(7))),
        [15, 18, 37]
    );
    world.patch("r15", vec![("n", attr(Value::U64(7)))], &[]);
    assert_eq!(ids_of(&world.branch, ns, &eq("n", Value::I64(7))), [18, 37]);
    assert_eq!(
        ids_of(&world.branch, ns, &eq("n", Value::U64(7))),
        [15, 21, 23]
    );
    assert_eq!(
        ids_of(
            &world.branch,
            ns,
            &Query::term(Term::exists(n(), TypeTag::U64))
        ),
        [15, 19, 20, 21, 22, 23, 38, 43]
    );
    check_queries(&world.branch, &world.model, ns, &probes);

    // Attribute -> field: the record keeps the value but leaves the index.
    world.patch("r02", vec![("n", field(s("a")))], &[]);
    assert_eq!(ids_of(&world.branch, ns, &eq("n", s("a"))), [10, 39]);
    // Field -> attribute: it joins.
    world.patch("r27", vec![("n", attr(s("a")))], &[]);
    assert_eq!(ids_of(&world.branch, ns, &eq("n", s("a"))), [10, 27, 39]);
    check_queries(&world.branch, &world.model, ns, &probes);

    // Removal and addition of a name.
    world.patch("r03", vec![], &["n"]);
    assert_eq!(
        ids_of(
            &world.branch,
            ns,
            &Query::term(Term::starts_with(n(), "ab"))
        ),
        [4, 9, 36, 41]
    );
    world.patch("r32", vec![("n", attr(Value::Bool(true)))], &[]);
    assert_eq!(
        ids_of(
            &world.branch,
            ns,
            &Query::term(Term::exists(n(), TypeTag::Bool))
        ),
        [24, 25, 26, 32, 40]
    );
    check_queries(&world.branch, &world.model, ns, &probes);

    // A patch touching two names: m changes type, k is removed.
    world.patch("r36", vec![("m", attr(s("abc")))], &["k"]);
    assert_eq!(
        ids_of(&world.branch, ns, &eq("m", Value::I64(7))),
        [15, 26, 45]
    );
    assert_eq!(
        ids_of(&world.branch, ns, &eq("m", s("abc"))),
        [12, 32, 36, 37]
    );
    assert_eq!(
        ids_of(
            &world.branch,
            ns,
            &Query::term(Term::exists(name("k"), TypeTag::Bool))
        ),
        [] as [u64; 0]
    );
    // Index no-ops: the same value again, a new field, an absent removal.
    let terms_before = world.branch.term_count(ns);
    world.patch("r14", vec![("m", attr(Value::I64(0)))], &[]);
    world.patch("r44", vec![("k", field(Value::bytes(b"\xff")))], &["q"]);
    assert_eq!(world.branch.term_count(ns), terms_before);
    check_queries(&world.branch, &world.model, ns, &probes);

    // Deletes: the records vanish from every source. r22 held the only
    // `n = U64 MAX`, so that value term is dropped, while its existence
    // term and r22's `m = I64 -1` (shared with r03 and r42) survive.
    assert_eq!(
        ids_of(&world.branch, ns, &eq("m", Value::I64(i64::MAX))),
        [5, 44]
    );
    world.delete("r05");
    assert_eq!(ids_of(&world.branch, ns, &eq("n", s("b"))), [] as [u64; 0]);
    assert_eq!(
        ids_of(&world.branch, ns, &eq("m", Value::I64(i64::MAX))),
        [44]
    );
    let terms_before = world.branch.term_count(ns);
    world.delete("r22");
    assert_eq!(
        world.branch.term_count(ns),
        terms_before.map(|count| count - 1)
    );
    assert_eq!(
        ids_of(&world.branch, ns, &eq("n", Value::U64(u64::MAX))),
        [] as [u64; 0]
    );
    assert_eq!(
        ids_of(
            &world.branch,
            ns,
            &Query::term(Term::exists(n(), TypeTag::U64))
        ),
        [15, 19, 20, 21, 23, 38, 43]
    );
    world.delete("r12");
    assert_eq!(
        ids_of(
            &world.branch,
            ns,
            &Query::term(Term::exists(m(), TypeTag::Str))
        ),
        [8, 11, 13, 24, 25, 28, 32, 36, 37, 39]
    );
    assert_eq!(
        world
            .branch
            .get_record(ns, &key("r12"), &Projection::ALL, Budget::unlimited())
            .expect("a valid read")
            .value,
        None
    );
    check_queries(&world.branch, &world.model, ns, &probes);

    // A recreated key gets a fresh id at the end of every order.
    assert_eq!(
        world.create(
            "r05",
            vec![("n", attr(s("b"))), ("m", attr(Value::U64(u64::MAX)))]
        ),
        rid(46)
    );
    assert_eq!(ids_of(&world.branch, ns, &eq("n", s("b"))), [46]);
    assert_eq!(
        world.create(
            "r46",
            vec![
                ("n", attr(Value::U64(u64::MAX))),
                ("m", attr(Value::I64(i64::MIN))),
                ("k", attr(s(""))),
            ]
        ),
        rid(47)
    );
    world.patch(
        "r18",
        vec![("n", attr(s("7"))), ("m", attr(Value::I64(7)))],
        &[],
    );
    assert_eq!(ids_of(&world.branch, ns, &eq("n", Value::I64(7))), [37]);
    assert_eq!(
        ids_of(&world.branch, ns, &eq("m", Value::I64(7))),
        [15, 18, 26, 45]
    );
    check_queries(
        &world.branch,
        &world.model,
        ns,
        &single_literal_queries(true),
    );
    check_queries(
        &world.branch,
        &world.model,
        ns,
        &combined_queries(&literal_pool(false)),
    );

    // Published, the state answers the same; the earlier snapshot is intact.
    let World { branch, model, .. } = world;
    let published = publish(&mut engine, branch);
    let after = engine.snapshot(published).expect("just published");
    assert_eq!(after.index_consistent(ns), Some(true));
    assert_eq!(after.typed_record_count(ns), Some(model.len()));
    check_queries(&after, &model, ns, &single_literal_queries(true));
    let first = engine
        .snapshot(fixture_snapshot)
        .expect("snapshot 1 is retained");
    assert_eq!(first.typed_record_count(ns), Some(FIXTURE_SIZE));
    check_queries(&first, &original, ns, &probes);
}

#[test]
fn deleted_records_disappear_and_a_rollback_brings_them_back() {
    let (engine, ns, model) = semantics_fixture();
    let mut world = World::begin(&engine, ns, model.clone());
    let query = eq("n", s("b"));
    let checkpoint = world.branch.checkpoint();

    world.delete("r05");
    world.delete("r44");
    assert_eq!(ids_of(&world.branch, ns, &query), [] as [u64; 0]);
    assert_eq!(
        world
            .branch
            .get_record(ns, &key("r05"), &Projection::KEY_ONLY, Budget::unlimited())
            .expect("a valid read")
            .value,
        None
    );
    assert_eq!(
        world
            .branch
            .count(ns, &Query::match_all(), Budget::unlimited())
            .expect("a valid count")
            .value,
        43
    );

    // Rolling back restores the rows, the bindings and the index together.
    world
        .branch
        .rollback_to(checkpoint)
        .expect("the checkpoint is valid");
    world.model = model;
    world.check_invariants();
    assert_eq!(ids_of(&world.branch, ns, &query), [5]);
    assert_eq!(
        world
            .branch
            .get_record(ns, &key("r05"), &Projection::KEY_ONLY, Budget::unlimited())
            .expect("a valid read")
            .value,
        Some(Row {
            id: rid(5),
            key: Some(key("r05")),
            cells: Vec::new(),
        })
    );
    check_queries(
        &world.branch,
        &world.model,
        ns,
        &single_literal_queries(false),
    );
    // A key recreated after the rollback still gets a fresh id: the
    // allocator was restored to 46, not to something lower.
    assert_eq!(world.create("r47", vec![("n", attr(s("b")))]), rid(46));
    assert_eq!(ids_of(&world.branch, ns, &query), [5, 46]);
}

/// Small limits so every fault is a few literals away.
const SMALL: Limits = Limits {
    max_cells: 3,
    max_str_len: 4,
    max_bytes_len: 4,
    max_page: 5,
    max_groups: 2,
    max_literals: 2,
    ..Limits::DEFAULT
};

/// Snapshot 1 of a SMALL-limits engine: namespace 1 with one record.
fn small_fixture() -> (Engine, NamespaceId) {
    let mut engine = Engine::with_limits(PriceSchedule::FIXTURE, SMALL);
    assert_eq!(engine.limits(), SMALL);
    let mut branch = begin(&engine, None);
    let ns = add_namespace(&mut branch, ALICE, "test");
    let cells = named(vec![("n", attr(s("abcd"))), ("m", attr(Value::I64(1)))]);
    apply(&mut branch, ALICE, create_record(ns, "one", cells));
    publish(&mut engine, branch);
    (engine, ns)
}

fn expect_fault<T: Debug>(result: ReadResult<T>, fault: QueryFault) {
    match result {
        Ok(metered) => panic!("expected {fault:?}, got {:?}", metered.value),
        Err(rejected) => {
            assert_eq!(rejected.reason, Rejection::InvalidQuery(fault));
            assert_eq!(
                rejected.receipt,
                read_rejected(0, Rejection::InvalidQuery(fault)),
                "{fault:?}: spent 0, receipt issued"
            );
        }
    }
}

fn lit(term: Term) -> Literal {
    Literal::of(term)
}

#[test]
fn query_faults_are_reported_in_the_validation_order_before_any_charge() {
    let (engine, ns) = small_fixture();
    let snapshot = engine.head_snapshot();
    let n = || name("n");
    let all = Projection::ALL;
    let query = |query: &Query, projection: &Projection, limit: usize| {
        snapshot.query(ns, query, projection, limit, None, Budget::unlimited())
    };
    // Faulty building blocks, each also carrying every later fault.
    let bytes_eq = || lit(Term::eq(n(), Value::bytes(b"x")));
    let long_eq = || lit(Term::eq(n(), s("abcde")));
    let unbounded = || lit(Term::range(n(), Bound::Unbounded, Bound::Unbounded));
    let too_wide = Projection::named(true, [name("a"), name("b"), name("c"), name("d")]);
    let wide_ok = Projection::named(true, [name("a"), name("b"), name("c")]);

    // 1. NoGroups masks a zero limit and an oversized projection.
    expect_fault(
        query(&Query { groups: vec![] }, &too_wide, 0),
        QueryFault::NoGroups,
    );
    // 2. TooManyGroups masks TooManyLiterals, the limit and the literals.
    let three_of = |literal: Literal| vec![literal.clone(), literal.clone(), literal];
    expect_fault(
        query(
            &Query {
                groups: vec![three_of(bytes_eq()), three_of(unbounded()), Vec::new()],
            },
            &too_wide,
            0,
        ),
        QueryFault::TooManyGroups,
    );
    // 3. TooManyLiterals masks the limit and the literals.
    expect_fault(
        query(&Query::all(three_of(bytes_eq())), &too_wide, 0),
        QueryFault::TooManyLiterals,
    );
    // 4. ZeroLimit and 5. LimitTooLarge mask the literals and the projection.
    expect_fault(
        query(&Query::all(vec![bytes_eq()]), &too_wide, 0),
        QueryFault::ZeroLimit,
    );
    expect_fault(
        query(&Query::all(vec![bytes_eq()]), &too_wide, 6),
        QueryFault::LimitTooLarge,
    );
    assert!(
        query(&Query::match_all(), &all, 5).is_ok(),
        "max_page is inclusive"
    );
    // 6. Literals in order: the first faulty literal decides, across groups.
    expect_fault(
        query(&Query::all(vec![bytes_eq(), unbounded()]), &too_wide, 5),
        QueryFault::NotIndexable,
    );
    expect_fault(
        query(&Query::all(vec![unbounded(), bytes_eq()]), &too_wide, 5),
        QueryFault::UnboundedRange,
    );
    expect_fault(
        query(
            &Query {
                groups: vec![vec![lit(Term::eq(n(), s("abcd")))], vec![long_eq()]],
            },
            &too_wide,
            5,
        ),
        QueryFault::ValueTooLarge,
    );
    // Every literal fault on its own.
    let faults: Vec<(Term, QueryFault)> = vec![
        (Term::eq(n(), Value::bytes(b"")), QueryFault::NotIndexable),
        (
            Term::not_eq(n(), Value::bytes(b"x")),
            QueryFault::NotIndexable,
        ),
        (Term::eq(n(), s("abcde")), QueryFault::ValueTooLarge),
        // `ä` is two bytes: five bytes here, four in the accepted list below.
        (Term::not_eq(n(), s("äbcd")), QueryFault::ValueTooLarge),
        (
            Term::range(n(), Bound::Unbounded, Bound::Unbounded),
            QueryFault::UnboundedRange,
        ),
        (
            Term::range(
                n(),
                Bound::Included(Value::I64(1)),
                Bound::Included(Value::U64(1)),
            ),
            QueryFault::MixedBounds,
        ),
        (
            Term::range(
                n(),
                Bound::Excluded(Value::U64(1)),
                Bound::Excluded(Value::bytes(b"x")),
            ),
            QueryFault::MixedBounds,
        ),
        // Every indexable type is ordered; only Bytes bounds are not. A Str
        // bound obeys the indexed-string limit like an equality value.
        (
            Term::range(
                n(),
                Bound::Included(s("abcdefgh")),
                Bound::Included(s("abcdefgh")),
            ),
            QueryFault::ValueTooLarge,
        ),
        (
            Term::range(n(), Bound::Unbounded, Bound::Excluded(s("äbcd"))),
            QueryFault::ValueTooLarge,
        ),
        (
            Term::range(
                n(),
                Bound::Unbounded,
                Bound::Included(Value::bytes(b"abcdefgh")),
            ),
            QueryFault::NotOrdered,
        ),
        (Term::starts_with(n(), "abcde"), QueryFault::ValueTooLarge),
        (Term::starts_with(n(), "äbcd"), QueryFault::ValueTooLarge),
        (Term::exists(n(), TypeTag::Bytes), QueryFault::NotIndexable),
    ];
    for (term, fault) in faults {
        expect_fault(query(&Query::term(term.clone()), &too_wide, 5), fault);
        // Negation and position do not change validation.
        expect_fault(
            query(
                &Query::all(vec![
                    lit(Term::exists(n(), TypeTag::Str)),
                    Literal::not(term),
                ]),
                &all,
                5,
            ),
            fault,
        );
    }
    // Values at the limit are accepted.
    for term in [
        Term::eq(n(), s("abcd")),
        Term::not_eq(n(), s("äbc")),
        Term::starts_with(n(), "äbc"),
        Term::range(
            n(),
            Bound::Excluded(Value::U64(0)),
            Bound::Excluded(Value::U64(0)),
        ),
        Term::range(n(), Bound::Included(s("a")), Bound::Excluded(s("abcd"))),
        Term::range(
            n(),
            Bound::Included(Value::Bool(false)),
            Bound::Excluded(Value::Bool(true)),
        ),
        Term::range(n(), Bound::Unbounded, Bound::Included(Value::Addr([1; 20]))),
    ] {
        assert!(query(&Query::term(term), &wide_ok, 5).is_ok());
    }
    // 7. ProjectionTooLarge comes after every literal. The cursor check, which
    // comes last, is covered in `pagination.rs`.
    expect_fault(
        query(&Query::match_all(), &too_wide, 5),
        QueryFault::ProjectionTooLarge,
    );

    // Validation precedes the directory lookup and ignores the budget.
    expect_fault(
        snapshot.query(
            vocab::ns(9),
            &Query::all(vec![bytes_eq()]),
            &all,
            5,
            None,
            Budget::limit(0),
        ),
        QueryFault::NotIndexable,
    );
    let unknown = snapshot
        .query(
            vocab::ns(9),
            &Query::match_all(),
            &all,
            5,
            None,
            Budget::unlimited(),
        )
        .expect_err("namespace 9 does not exist");
    assert_eq!(unknown.reason, Rejection::UnknownNamespace);
    assert_eq!(
        unknown.receipt,
        read_rejected(1, Rejection::UnknownNamespace),
        "DirectoryLookup is the only charge"
    );
    // A traced zero-cost receipt carries an empty ledger.
    let rejected = snapshot
        .query(
            ns,
            &Query { groups: vec![] },
            &all,
            5,
            None,
            Budget::unlimited().traced(),
        )
        .expect_err("no groups");
    assert_eq!(
        rejected.receipt,
        traced(
            read_rejected(0, Rejection::InvalidQuery(QueryFault::NoGroups)),
            ledger(&[])
        )
    );
}

#[test]
fn count_and_get_record_validate_like_query() {
    let (engine, ns) = small_fixture();
    let snapshot = engine.head_snapshot();
    let n = || name("n");
    let count = |query: &Query| snapshot.count(ns, query, Budget::unlimited());
    let three = |literal: Literal| vec![literal.clone(), literal.clone(), literal];
    let bytes_eq = lit(Term::eq(n(), Value::bytes(b"x")));

    expect_fault(count(&Query { groups: vec![] }), QueryFault::NoGroups);
    expect_fault(
        count(&Query {
            groups: vec![three(bytes_eq.clone()), Vec::new(), Vec::new()],
        }),
        QueryFault::TooManyGroups,
    );
    expect_fault(
        count(&Query::all(three(bytes_eq.clone()))),
        QueryFault::TooManyLiterals,
    );
    // The implicit limit of 1 is valid, so the literals are next.
    expect_fault(count(&Query::all(vec![bytes_eq])), QueryFault::NotIndexable);
    expect_fault(
        count(&Query::term(Term::range(
            n(),
            Bound::Included(Value::I64(0)),
            Bound::Included(Value::U64(0)),
        ))),
        QueryFault::MixedBounds,
    );
    expect_fault(
        count(&Query::term(Term::starts_with(n(), "abcde"))),
        QueryFault::ValueTooLarge,
    );
    assert_eq!(count(&Query::match_all()).expect("a valid count").value, 1);

    // get_record checks only the projection.
    let too_wide = Projection::named(false, [name("a"), name("b"), name("c"), name("d")]);
    expect_fault(
        snapshot.get_record(ns, &key("one"), &too_wide, Budget::unlimited()),
        QueryFault::ProjectionTooLarge,
    );
    expect_fault(
        snapshot.get_record(vocab::ns(9), &key("one"), &too_wide, Budget::limit(0)),
        QueryFault::ProjectionTooLarge,
    );
    let wide_ok = Projection::named(false, [name("a"), name("b"), name("m")]);
    assert_eq!(
        snapshot
            .get_record(ns, &key("one"), &wide_ok, Budget::unlimited())
            .expect("three names are allowed")
            .value,
        Some(Row {
            id: rid(1),
            key: None,
            cells: vec![(name("m"), attr(Value::I64(1)))],
        })
    );
}

/// A snapshot's own cursor on an unknown namespace is refused before the
/// directory lookup. The rest of the cursor-binding matrix is in
/// `pagination.rs`.
#[test]
fn a_cursor_on_an_unknown_namespace_is_a_mismatch_before_any_lookup() {
    let (engine, ns, _) = semantics_fixture();
    let first = engine.head_snapshot();
    let query = Query::term(Term::exists(name("n"), TypeTag::Str));
    let cursor = first
        .query(ns, &query, &Projection::ALL, 2, None, Budget::unlimited())
        .expect("a valid query")
        .value
        .next
        .expect("fifteen Str attributes of n");
    let rejected = first
        .query(
            vocab::ns(9),
            &query,
            &Projection::ALL,
            5,
            Some(&cursor),
            Budget::unlimited(),
        )
        .expect_err("cursor mismatch");
    assert_eq!(rejected.reason, Rejection::CursorMismatch);
    assert_eq!(
        rejected.receipt,
        read_rejected(0, Rejection::CursorMismatch)
    );
}

/// Snapshot 1 of a separate engine: namespace 1 with, in id order,
///
/// | id | key  | kind (Str) | price       |
/// | -- | ---- | ---------- | ----------- |
/// | 1  | `k1` | `book`     | I64 10      |
/// | 2  | `k2` | `book`     | I64 20      |
/// | 3  | `k3` | `pen`      | I64 5       |
/// | 4  | `k4` | `pen`      | I64 20      |
/// | 5  | `k5` | `ink`      | U64 20      |
/// | 6  | `k6` | (absent)   | I64 30      |
///
/// Term keys in index order: kind/Str/∃, `book`, `ink`, `pen`; price/I64/∃,
/// 5, 10, 20, 30; price/U64/∃, 20.
fn cost_fixture() -> (Engine, NamespaceId) {
    let mut engine = Engine::new(PriceSchedule::FIXTURE);
    let mut branch = begin(&engine, None);
    let ns = add_namespace(&mut branch, ALICE, "test");
    let records = [
        (
            "k1",
            vec![("kind", attr(s("book"))), ("price", attr(Value::I64(10)))],
        ),
        (
            "k2",
            vec![("kind", attr(s("book"))), ("price", attr(Value::I64(20)))],
        ),
        (
            "k3",
            vec![("kind", attr(s("pen"))), ("price", attr(Value::I64(5)))],
        ),
        (
            "k4",
            vec![("kind", attr(s("pen"))), ("price", attr(Value::I64(20)))],
        ),
        (
            "k5",
            vec![("kind", attr(s("ink"))), ("price", attr(Value::U64(20)))],
        ),
        ("k6", vec![("price", attr(Value::I64(30)))]),
    ];
    for (key_text, cells) in records {
        apply(
            &mut branch,
            ALICE,
            create_record(ns, key_text, named(cells)),
        );
    }
    publish(&mut engine, branch);
    assert_eq!(engine.head_snapshot().term_count(ns), Some(11));
    (engine, ns)
}

/// Runs one unlimited page of ids and returns `(ids, receipt)`.
fn id_page(
    snapshot: &Snapshot,
    ns: NamespaceId,
    query: &Query,
    projection: &Projection,
    budget: Budget,
) -> (Vec<u64>, Receipt) {
    let metered = snapshot
        .query(ns, query, projection, 10, None, budget)
        .unwrap_or_else(|rejected| panic!("{query:?}: {rejected}"));
    assert_eq!(metered.value.next, None, "{query:?}");
    (ordinals(&metered.value.rows), metered.receipt)
}

#[test]
fn a_cell_of_another_type_or_an_absent_cell_is_charged_only_where_it_is_visited() {
    let (engine, ns) = cost_fixture();
    let snapshot = engine.head_snapshot();
    let unlimited = Budget::unlimited();

    // Everything from 5 up: 4 term visits (5, 10, 20, 30); the U64 20 of
    // record 5 sits under another tag and is neither visited nor returned:
    // 1 + 5 + 4 + 12 + PostingEntry 5 + ResultRow 15 = 42.
    let query = Query::term(Term::range(
        name("price"),
        Bound::Included(Value::I64(5)),
        Bound::Unbounded,
    ));
    let (ids, receipt) = id_page(&snapshot, ns, &query, &Projection::ID_ONLY, unlimited);
    assert_eq!((ids, receipt), (vec![1, 2, 3, 4, 6], read_applied(42)));

    // NotEq(price, I64 20): record 5 (U64) is not even a candidate.
    let query = Query::term(Term::not_eq(name("price"), Value::I64(20)));
    let (ids, receipt) = id_page(&snapshot, ns, &query, &Projection::ID_ONLY, unlimited);
    assert_eq!((ids, receipt), (vec![1, 3, 6], read_applied(34)));

    // NOT Eq(kind, book) with the key: no seek; every live row is visited
    // (RowVisit 6) and probed (RecordProbe 12); 3, 4, 5 and 6 (no kind at
    // all) pass; ResultRow 12 + ReadByte 2 per key = 20: 1 + 5 + 18 + 20 = 44.
    let query = Query::all(vec![Literal::not(Term::eq(name("kind"), s("book")))]);
    let (ids, receipt) = id_page(
        &snapshot,
        ns,
        &query,
        &Projection::KEY_ONLY,
        unlimited.traced(),
    );
    assert_eq!(ids, [3, 4, 5, 6]);
    assert_eq!(
        receipt,
        traced(
            read_applied(44),
            ledger(&[
                (OperationClass::DirectoryLookup, 1, 1, 1),
                (OperationClass::QueryGroup, 1, 5, 5),
                (OperationClass::RowVisit, 6, 1, 6),
                (OperationClass::RecordProbe, 6, 2, 12),
                (OperationClass::ResultRow, 4, 3, 12),
                (OperationClass::ReadByte, 8, 1, 8),
            ])
        )
    );
}
