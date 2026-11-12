//! Independent model test: generated block histories of typed records, term
//! indexes, queries and host expiry, checked against an oracle.
//!
//! For each fixed seed the workload runs 120 real blocks on one engine with
//! small instance limits (6 cells, 16-byte values, pages of 5) over keys of
//! 1 to 32 bytes: creates, patches, deletes, key recreation, transactions
//! with checkpoint and rollback, admission faults, budget failures,
//! host-path writes, a stale losing candidate, admin pricing candidates,
//! empty blocks and host expiry. Every 20th block stamps all of its keys to
//! expire in the next block, whose expiry first runs with a starved delete
//! budget (a halt) and then over several pages; every tenth block gets the
//! starved run too.
//!
//! The oracle keeps its own mutation log and BTreeMap views and predicts,
//! before looking at the engine, every verdict, effect, record id, write
//! receipt (from the charge points below and the unit prices in
//! `support/pricing.rs`) and expiry. Reads at every snapshot
//! are checked against a full-scan reference evaluator; single-group query
//! and count receipts are predicted, multi-group queries are checked for
//! results only. Raw `Put`/`Delete` histories are covered by `history.rs`.

use std::{
    collections::{BTreeMap, BTreeSet, HashSet},
    ops::{Bound, Range},
};

use arkiv_db::{
    ArgumentFault, BlockNumber, Branch, Budget, Cell, CellName, Checkpoint, Cost, Cursor,
    DEFAULT_NAMESPACE_CAP, Effect, Engine, EngineError, Limits, Literal, NamespaceId,
    OperationClass, Outcome, OwnerId, PriceSchedule, Projection, Query, Receipt, ReceiptKind,
    RecordId, RecordKey, Rejection, Row, SealedCandidate, Snapshot, SnapshotId, Term, TypeTag,
    Value, WriteOp, WriteResult, expire_records,
};

use crate::support::{
    model::{Record, cell_bytes, project, query_holds, term_holds},
    oracle::{
        Ctx, Flat, Run, Transcript, Weights, assert_replay, assert_verdicts_seen, block_size,
        count, draw_budget, expected_read, flat_read, flat_write, publish_checked,
        publish_pricing_candidate, receipt_of, rejection, schedule_at, seal_checked, seed_tests,
        total, verdict, verdict_name,
    },
    random::Generator,
    reader::Reader,
    vocab::{
        ALICE, BOB, CAROL, Cells, applied, attr, create_namespace, create_record, delete_record,
        field, key, name, ns, patch_record, put, read_applied, receipt, rid,
    },
};

const SEEDS: [u32; 3] = [0x0000_0007, 0x9e37_79b9, 0xdead_beef];
const BLOCKS: u64 = 120;
/// Block sizes cycle through these from block 2 on; block 1 seeds the two
/// namespaces.
const SIZES: [usize; 6] = [0, 1, 4, 8, 16, 24];
const ADMIN_EVERY: u64 = 25;
const LOSER_EVERY: u64 = 13;
/// Blocks whose expiry first runs with a starved delete budget.
const STARVE_EVERY: u64 = 10;
const TRANSACTION_OPS: usize = 5;
/// Page size of every cursor walk; also the instance's `max_page`.
const PAGE: usize = 5;

/// Instance limits, small enough that `RecordFull`, `TooManyCells`,
/// `ValueTooLarge` and multi-page expiry are reachable by the workload.
const LIMITS: Limits = Limits {
    namespace_cap: DEFAULT_NAMESPACE_CAP,
    max_cells: 6,
    max_str_len: 16,
    max_bytes_len: 16,
    max_page: PAGE,
    max_groups: 8,
    max_literals: 8,
    ..Limits::DEFAULT
};

const OWNERS: [OwnerId; 3] = [ALICE, BOB, CAROL];

/// Namespace 1 is alice's, namespace 2 is bob's; namespace 3 never exists.
const NS1: u64 = 1;
const NS2: u64 = 2;
const SENTINEL: u64 = 3;

/// Every record key the workload uses; `get_record` is checked for all of
/// them (plus an absent key) at every snapshot. The last one is 32 bytes,
/// the longest key (the width of an SDK entity key).
const KEYS: [&[u8]; 19] = [
    b"a",
    b"b",
    b"k00",
    b"k01",
    b"k02",
    b"k03",
    b"k04",
    b"k05",
    b"k06",
    b"k07",
    b"k08",
    b"k09",
    b"k10",
    b"k11",
    b"k12",
    b"k13",
    b"k14",
    b"k15",
    b"long-key-with-thirty-two-bytes!!",
];
const ABSENT_KEY: &[u8] = b"absent";

const KIND: &str = "kind";
const PRICE: &str = "price";
const QTY: &str = "qty";
const FLAG: &str = "flag";
const TAG: &str = "tag";
const NOTE: &str = "note";
const EXPIRES: &str = "$expiresAt";
/// A name no record ever holds.
const GHOST: &str = "ghost";

const KINDS: [&str; 3] = ["book", "pen", "cup"];
const PRICES: [i64; 5] = [-5, 0, 10, 20, 35];
const QTYS: [u64; 4] = [0, 1, 7, 300];
const TAGS: [&str; 4] = ["aa", "ab", "b", "abc"];
const NOTES: [&[u8]; 3] = [b"xyz", b"", b"\x00\xff"];

type CellMap = BTreeMap<CellName, Cell>;

fn cells_of(list: &[(CellName, Cell)]) -> CellMap {
    list.iter().cloned().collect()
}

/// The charge points of the typed record operations (see docs/metering.md).
/// Every element of a charge list is one atomic charge group in charge order.
impl Weights {
    /// One index insert: `{IndexInsert 1, IndexTermCreate 1 if new,
    /// RootUpdate 2}`.
    const fn insert(&self, new: bool) -> u64 {
        let base = self.index_insert + 2 * self.root_update;
        if new {
            base + self.index_term_create
        } else {
            base
        }
    }

    /// One index removal: `{IndexRemove 1, RootUpdate 2}`.
    const fn remove(&self) -> u64 {
        self.index_remove + 2 * self.root_update
    }

    /// CreateRecord: DirectoryLookup; PointRead; {RecordCreate 1, CellWrite c,
    /// CellByte b, RowByte n, RootUpdate 3}; per attribute cell in name
    /// order: value term insert, existence term insert. `new` decides
    /// IndexTermCreate per term. `n` = key bytes + b.
    fn create_record_charges(&self, model: &NsModel, key: &RecordKey, cells: &CellMap) -> Vec<u64> {
        let bytes: u64 = cells
            .iter()
            .map(|(name, cell)| cell_bytes(name, cell))
            .sum();
        let row = count(key.as_bytes().len()) + bytes;
        let mut charges = vec![
            self.directory_lookup,
            self.point_read,
            self.record_create
                + self.cell_write * count(cells.len())
                + self.cell_byte * bytes
                + self.row_byte * row
                + 3 * self.root_update,
        ];
        for (name, cell) in cells {
            if cell.is_attribute() {
                charges.push(self.insert(!model.holds_value(name, &cell.value, None)));
                charges.push(self.insert(!model.holds_existence(name, cell.value.tag(), None)));
            }
        }
        charges
    }

    /// PatchRecord: DirectoryLookup; PointRead; {RecordPatch 1, CellWrite s,
    /// CellByte b, CellRemove r, RowByte n, RootUpdate 2}; per name in sorted
    /// order over set ∪ remove: a removal per term in old − new (value
    /// first), then an insert per term in new − old (value first). Newness
    /// excludes the patched record. `n` = key bytes + Σ over the resulting
    /// cells of (name bytes + encoded value bytes). An empty patch charges
    /// DirectoryLookup, PointRead and RecordPatch 1 only.
    fn patch_record_charges(
        &self,
        model: &NsModel,
        id: RecordId,
        key: &RecordKey,
        old: &CellMap,
        set: &[(CellName, Cell)],
        remove: &[CellName],
    ) -> Vec<u64> {
        if set.is_empty() && remove.is_empty() {
            // golem-db's no-op: the lookups and the patch base charge only.
            return vec![self.directory_lookup, self.point_read, self.record_patch];
        }
        let bytes: u64 = set.iter().map(|(name, cell)| cell_bytes(name, cell)).sum();
        let new_cells = patched_cells(old, set, remove);
        let row = count(key.as_bytes().len())
            + new_cells
                .iter()
                .map(|(name, cell)| cell_bytes(name, cell))
                .sum::<u64>();
        let mut charges = vec![
            self.directory_lookup,
            self.point_read,
            self.record_patch
                + self.cell_write * count(set.len())
                + self.cell_byte * bytes
                + self.cell_remove * count(remove.len())
                + self.row_byte * row
                + 2 * self.root_update,
        ];
        let names: BTreeSet<&CellName> = set
            .iter()
            .map(|(name, _)| name)
            .chain(remove.iter())
            .collect();
        for name in names {
            let before = old.get(name).map_or_else(Vec::new, terms_of);
            let after = new_cells.get(name).map_or_else(Vec::new, terms_of);
            for _ in before.iter().filter(|term| !after.contains(term)) {
                charges.push(self.remove());
            }
            for term in after.iter().filter(|term| !before.contains(term)) {
                let held = match term {
                    TermId::Value(value) => model.holds_value(name, value, Some(id)),
                    TermId::Existence(tag) => model.holds_existence(name, *tag, Some(id)),
                };
                charges.push(self.insert(!held));
            }
        }
        charges
    }

    /// DeleteRecord: DirectoryLookup; PointRead; {RecordDelete 1, CellRemove
    /// c, RootUpdate 3}; per attribute cell in name order: a removal (value),
    /// a removal (existence).
    fn delete_record_charges(&self, cells: &CellMap) -> Vec<u64> {
        let mut charges = vec![
            self.directory_lookup,
            self.point_read,
            self.record_delete + self.cell_remove * count(cells.len()) + 3 * self.root_update,
        ];
        for cell in cells.values() {
            if cell.is_attribute() {
                charges.push(self.remove());
                charges.push(self.remove());
            }
        }
        charges
    }

    /// get_record: DirectoryLookup; PointRead; if bound {ResultRow 1,
    /// ReadByte p}.
    fn get_record_charges(&self, projected: Option<u64>) -> Vec<u64> {
        let mut charges = vec![self.directory_lookup, self.point_read];
        if let Some(bytes) = projected {
            charges.push(self.result_row + self.read_byte * bytes);
        }
        charges
    }
}

/// The admin schedule committed by the `n`-th admin candidate. Both halves
/// are written from the same literals; the oracle never reads weights back.
fn admin_schedule(n: u32, id: u32) -> (PriceSchedule, Weights) {
    match n % 3 {
        0 => (PriceSchedule::uniform(id, 3), Weights::uniform(id, 3)),
        1 => (
            PriceSchedule::FIXTURE
                .with_id(id)
                .with_price(OperationClass::IndexTermCreate, 7)
                .with_price(OperationClass::PostingEntry, 2)
                .with_price(OperationClass::RecordProbe, 5)
                .with_price(OperationClass::ReadByte, 2),
            Weights {
                id,
                index_term_create: 7,
                posting_entry: 2,
                record_probe: 5,
                read_byte: 2,
                ..Weights::FIXTURE
            },
        ),
        _ => (PriceSchedule::uniform(id, 1), Weights::uniform(id, 1)),
    }
}

/// An index term of one name: the value term or the existence term.
#[derive(Clone, Debug, PartialEq, Eq)]
enum TermId {
    Value(Value),
    Existence(TypeTag),
}

/// The terms an attribute cell holds, value term first; a field holds none.
fn terms_of(cell: &Cell) -> Vec<TermId> {
    if cell.is_attribute() {
        vec![
            TermId::Value(cell.value.clone()),
            TermId::Existence(cell.value.tag()),
        ]
    } else {
        Vec::new()
    }
}

/// The cells a patch leaves: `set` replaces or adds, `remove` drops.
fn patched_cells(old: &CellMap, set: &[(CellName, Cell)], remove: &[CellName]) -> CellMap {
    let mut cells = old.clone();
    for (name, cell) in set {
        cells.insert(name.clone(), cell.clone());
    }
    for name in remove {
        cells.remove(name);
    }
    cells
}

fn cells_fault(cells: &[(CellName, Cell)]) -> Option<ArgumentFault> {
    if cells.len() > LIMITS.max_cells {
        return Some(ArgumentFault::TooManyCells);
    }
    let mut seen = BTreeSet::new();
    for (name, cell) in cells {
        if !seen.insert(name) {
            return Some(ArgumentFault::DuplicateCell);
        }
        let long = match &cell.value {
            Value::Str(text) => text.len() > LIMITS.max_str_len,
            Value::Bytes(bytes) => bytes.len() > LIMITS.max_bytes_len,
            _ => false,
        };
        if long {
            return Some(ArgumentFault::ValueTooLarge);
        }
        if cell.is_attribute() && matches!(cell.value, Value::Bytes(_)) {
            return Some(ArgumentFault::BytesNotIndexable);
        }
    }
    None
}

fn argument_fault(op: &WriteOp) -> Option<ArgumentFault> {
    match op {
        WriteOp::CreateRecord { cells, .. } => cells_fault(cells),
        WriteOp::PatchRecord { set, remove, .. } => {
            if set.len() + remove.len() > LIMITS.max_cells {
                return Some(ArgumentFault::TooManyCells);
            }
            if let Some(fault) = cells_fault(set) {
                return Some(fault);
            }
            let mut removed = BTreeSet::new();
            for name in remove {
                if !removed.insert(name) {
                    return Some(ArgumentFault::DuplicateCell);
                }
            }
            if set.iter().any(|(name, _)| removed.contains(name)) {
                return Some(ArgumentFault::SetAndRemoveOverlap);
            }
            None
        }
        _ => None,
    }
}

// The model: per-namespace live rows, queried with the full-scan reference
// evaluator of `support/model.rs`, never an index.

#[derive(Clone, Debug, PartialEq, Eq)]
struct NsModel {
    owner: OwnerId,
    rows: BTreeMap<RecordId, Record>,
    keys: BTreeMap<RecordKey, RecordId>,
    next_id: u64,
}

impl NsModel {
    fn new(owner: OwnerId) -> Self {
        Self {
            owner,
            rows: BTreeMap::new(),
            keys: BTreeMap::new(),
            next_id: 1,
        }
    }

    fn bound(&self, key: &RecordKey) -> Option<(RecordId, &Record)> {
        let id = *self.keys.get(key)?;
        Some((id, &self.rows[&id]))
    }

    /// Whether a live record other than `except` holds attribute `name`
    /// with exactly this type and value.
    fn holds_value(&self, name: &CellName, value: &Value, except: Option<RecordId>) -> bool {
        self.rows.iter().any(|(id, rec)| {
            Some(*id) != except
                && attribute(rec, name)
                    .is_some_and(|held| held.tag() == value.tag() && held == value)
        })
    }

    /// Whether a live record other than `except` holds an attribute `name`
    /// of type `tag`.
    fn holds_existence(&self, name: &CellName, tag: TypeTag, except: Option<RecordId>) -> bool {
        self.rows.iter().any(|(id, rec)| {
            Some(*id) != except && attribute(rec, name).is_some_and(|held| held.tag() == tag)
        })
    }

    /// Number of distinct index terms the live rows imply: a value term and
    /// an existence term per attribute.
    fn term_count(&self) -> usize {
        let mut terms = HashSet::new();
        for rec in self.rows.values() {
            for (name, cell) in &rec.cells {
                if cell.is_attribute() {
                    terms.insert((name, cell.value.tag(), Some(&cell.value)));
                    terms.insert((name, cell.value.tag(), None));
                }
            }
        }
        terms.len()
    }

    /// Every live record matching `query`, in ascending id order.
    fn matches(&self, query: &Query) -> Vec<RecordId> {
        self.rows
            .iter()
            .filter(|(_, rec)| query_holds(query, &rec.cells))
            .map(|(id, _)| *id)
            .collect()
    }

    fn apply(&mut self, mutation: &Mutation) {
        match mutation {
            Mutation::Create { key, id, cells } => {
                assert_eq!(id.ordinal(), self.next_id, "ids are dense");
                assert!(
                    self.keys.insert(key.clone(), *id).is_none(),
                    "key was bound"
                );
                self.rows.insert(
                    *id,
                    Record {
                        key: key.clone(),
                        cells: cells.clone(),
                    },
                );
                self.next_id += 1;
            }
            Mutation::Patch { id, set, remove } => {
                let rec = self.rows.get_mut(id).expect("patched rows are live");
                rec.cells = patched_cells(&rec.cells, set, remove);
            }
            Mutation::Delete { id } => {
                let rec = self.rows.remove(id).expect("deleted rows are live");
                assert_eq!(self.keys.remove(&rec.key), Some(*id));
            }
        }
    }
}

fn attribute<'r>(rec: &'r Record, name: &CellName) -> Option<&'r Value> {
    rec.cells
        .get(name)
        .filter(|cell| cell.is_attribute())
        .map(|cell| &cell.value)
}

/// The bytes of a projected row that `ReadByte` counts: its cells and, when
/// projected, its key.
fn row_bytes(row: &Row) -> u64 {
    let key = row.key.as_ref().map_or(0, |key| key.as_bytes().len());
    let cells: u64 = row.cells.iter().map(|(n, c)| cell_bytes(n, c)).sum();
    count(key) + cells
}

// Single-group query charges, from the lazy evaluation rules. The source and
// probe list follow the first literal; every candidate and probe is charged
// as it is revealed; the page stops at `limit` rows with the next head
// already pulled.

/// A predicted page or count walk: charge points in order plus the emitted
/// ids and the continuation.
#[derive(Debug, PartialEq, Eq)]
struct Plan {
    charges: Vec<u64>,
    rows: Vec<RecordId>,
    next: Option<RecordId>,
}

fn single_group(query: &Query) -> Option<&[Literal]> {
    match query.groups.as_slice() {
        [group] => Some(group.as_slice()),
        _ => None,
    }
}

/// Opens one AND-group after `after`: QueryGroup, the seek and the run's
/// term visits, and returns the candidate ids in id order, the per-candidate
/// charge (PostingEntry or RowVisit) and the probe list.
fn open_group<'q>(
    w: &Weights,
    model: &NsModel,
    group: &'q [Literal],
    after: Option<RecordId>,
    charges: &mut Vec<u64>,
) -> (Vec<RecordId>, u64, Vec<&'q Literal>) {
    charges.push(w.query_group);
    let live = |keep: &dyn Fn(&Record) -> bool| -> Vec<RecordId> {
        model
            .rows
            .iter()
            .filter(|(id, _)| after.is_none_or(|a| **id > a))
            .filter(|(_, rec)| keep(rec))
            .map(|(id, _)| *id)
            .collect()
    };
    let Some(first) = group.first() else {
        return (live(&|_| true), w.row_visit, Vec::new());
    };
    let mut probes: Vec<&'q Literal> = Vec::new();
    if first.negated {
        probes.push(first);
        probes.extend(&group[1..]);
        return (live(&|_| true), w.row_visit, probes);
    }
    charges.push(w.term_seek);
    let holds = |rec: &Record| term_holds(&first.term, &rec.cells);
    let candidates = match &first.term {
        Term::Eq { .. } | Term::Exists { .. } => live(&holds),
        Term::NotEq { name, value } => {
            probes.push(first);
            live(&|rec| attribute(rec, name).is_some_and(|held| held.tag() == value.tag()))
        }
        Term::Range { name, .. } | Term::StartsWith { name, .. } => {
            // TermVisit per distinct term key the run covers: the values
            // held by any live row, cursor or not.
            let visited: HashSet<&Value> = model
                .rows
                .values()
                .filter(|rec| holds(rec))
                .filter_map(|rec| attribute(rec, name))
                .collect();
            charges.extend(std::iter::repeat_n(w.term_visit, visited.len()));
            live(&holds)
        }
    };
    probes.extend(&group[1..]);
    (candidates, w.posting_entry, probes)
}

/// Pulls the group's next head: each candidate is charged as revealed, then
/// probed in order until one fails.
fn pull(
    w: &Weights,
    model: &NsModel,
    candidates: &mut std::vec::IntoIter<RecordId>,
    per_candidate: u64,
    probes: &[&Literal],
    charges: &mut Vec<u64>,
) -> Option<RecordId> {
    for id in candidates.by_ref() {
        charges.push(per_candidate);
        let cells = &model.rows[&id].cells;
        let mut passed = true;
        for probe in probes {
            charges.push(w.record_probe);
            if term_holds(&probe.term, cells) == probe.negated {
                passed = false;
                break;
            }
        }
        if passed {
            return Some(id);
        }
    }
    None
}

/// One page of a single-group query after `after`.
fn predict_page(
    w: &Weights,
    model: &NsModel,
    group: &[Literal],
    projection: &Projection,
    limit: usize,
    after: Option<RecordId>,
) -> Plan {
    let mut charges = vec![w.directory_lookup];
    let (candidates, per_candidate, probes) = open_group(w, model, group, after, &mut charges);
    let mut candidates = candidates.into_iter();
    let mut head = pull(
        w,
        model,
        &mut candidates,
        per_candidate,
        &probes,
        &mut charges,
    );
    let mut rows: Vec<RecordId> = Vec::new();
    loop {
        let Some(id) = head else {
            return Plan {
                charges,
                rows,
                next: None,
            };
        };
        if rows.len() == limit {
            let last = rows.last().copied();
            return Plan {
                charges,
                rows,
                next: last,
            };
        }
        let bytes = row_bytes(&project(id, &model.rows[&id], projection));
        charges.push(w.result_row + w.read_byte * bytes);
        rows.push(id);
        head = pull(
            w,
            model,
            &mut candidates,
            per_candidate,
            &probes,
            &mut charges,
        );
    }
}

/// A single-group count: the same enumeration without row charges.
fn predict_count(w: &Weights, model: &NsModel, group: &[Literal]) -> Plan {
    let mut charges = vec![w.directory_lookup];
    let (candidates, per_candidate, probes) = open_group(w, model, group, None, &mut charges);
    let mut candidates = candidates.into_iter();
    let mut rows = Vec::new();
    while let Some(id) = pull(
        w,
        model,
        &mut candidates,
        per_candidate,
        &probes,
        &mut charges,
    ) {
        rows.push(id);
    }
    Plan {
        charges,
        rows,
        next: None,
    }
}

// Oracle: the published mutation log per namespace, views replayed from it,
// and the open candidate.

#[derive(Clone, Debug, PartialEq, Eq)]
enum Mutation {
    Create {
        key: RecordKey,
        id: RecordId,
        cells: CellMap,
    },
    Patch {
        id: RecordId,
        set: Cells,
        remove: Vec<CellName>,
    },
    Delete {
        id: RecordId,
    },
}

type View = BTreeMap<NamespaceId, NsModel>;

#[derive(Default)]
struct Log {
    created: Vec<(u64, NamespaceId, OwnerId)>,
    mutations: Vec<(u64, NamespaceId, Mutation)>,
    /// The schedule in force for blocks from this one on.
    schedules: Vec<(u64, Weights)>,
}

impl Log {
    /// The view after `block`, replayed from the log alone.
    fn view_at(&self, block: u64) -> View {
        let mut view = View::new();
        for &(at, namespace, owner) in &self.created {
            if at <= block {
                view.insert(namespace, NsModel::new(owner));
            }
        }
        for (at, namespace, mutation) in &self.mutations {
            if *at <= block {
                view.get_mut(namespace)
                    .expect("mutations follow creation")
                    .apply(mutation);
            }
        }
        view
    }
}

struct Oracle {
    log: Log,
    /// The log projected to the head, recomputed at every commit.
    head: View,
}

impl Oracle {
    fn new() -> Self {
        let mut log = Log::default();
        log.schedules.push((0, Weights::FIXTURE));
        Self {
            log,
            head: View::new(),
        }
    }

    /// Publishes the candidate under `block` and returns the namespaces it
    /// mutated.
    fn commit(&mut self, block: u64, pending: Pending) -> BTreeSet<NamespaceId> {
        let touched: BTreeSet<NamespaceId> = pending.mutations.iter().map(|(n, _)| *n).collect();
        for (namespace, owner) in pending.created {
            self.log.created.push((block, namespace, owner));
        }
        for (namespace, mutation) in pending.mutations {
            self.log.mutations.push((block, namespace, mutation));
        }
        self.head = self.log.view_at(u64::MAX);
        assert_eq!(
            self.head, pending.view,
            "block {block}: the replayed log differs from the incremental view"
        );
        touched
    }
}

struct Mark {
    view: View,
    mutations: usize,
    receipts: usize,
}

/// Which authority a write runs under.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Path {
    Owner(OwnerId),
    Host,
}

/// The open candidate as the oracle sees it.
struct Pending {
    weights: Weights,
    view: View,
    created: Vec<(NamespaceId, OwnerId)>,
    mutations: Vec<(NamespaceId, Mutation)>,
    marks: Vec<Mark>,
    /// Receipts the branch must have recorded, in order.
    receipts: Vec<Receipt>,
    block_remaining: u64,
}

impl Pending {
    fn open(oracle: &Oracle, weights: Weights, block_budget: Option<u64>) -> Self {
        Self {
            weights,
            view: oracle.head.clone(),
            created: Vec::new(),
            mutations: Vec::new(),
            marks: Vec::new(),
            receipts: Vec::new(),
            block_remaining: block_budget.unwrap_or(u64::MAX),
        }
    }

    fn spent(&self) -> u64 {
        self.receipts.iter().map(|r| r.spent.units()).sum()
    }

    fn checkpoint(&mut self) {
        self.marks.push(Mark {
            view: self.view.clone(),
            mutations: self.mutations.len(),
            receipts: self.receipts.len(),
        });
    }

    /// Restores the view of mark `index` (rows, keys and the id allocator)
    /// and returns the discarded receipt range and the number of creations
    /// whose ids were released.
    fn rollback(&mut self, index: usize) -> (Range<usize>, usize) {
        let mark = &self.marks[index];
        let released = self.mutations[mark.mutations..]
            .iter()
            .filter(|(_, m)| matches!(m, Mutation::Create { .. }))
            .count();
        let discarded = mark.receipts..self.receipts.len();
        self.view = mark.view.clone();
        self.mutations.truncate(mark.mutations);
        self.marks.truncate(index + 1);
        (discarded, released)
    }

    /// Records a write receipt against the block budget and returns it.
    fn record(&mut self, outcome: Outcome, spent: u64) -> Receipt {
        self.block_remaining -= spent;
        let receipt = receipt(ReceiptKind::Write, self.weights.id, spent, outcome);
        self.receipts.push(receipt.clone());
        receipt
    }

    fn mutate(&mut self, namespace: NamespaceId, mutation: Mutation) {
        self.view
            .get_mut(&namespace)
            .expect("mutations target known namespaces")
            .apply(&mutation);
        self.mutations.push((namespace, mutation));
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Predicted {
    /// Admission failure: spent 0, receipt issued, nothing recorded.
    NotAdmitted(Rejection),
    /// A recorded receipt; the effect and mutation are present exactly when
    /// the op applied.
    Recorded {
        outcome: Outcome,
        spent: u64,
        applied: Option<(Effect, Mutation)>,
    },
}

fn op_namespace(op: &WriteOp) -> NamespaceId {
    match op {
        WriteOp::CreateRecord { namespace, .. }
        | WriteOp::PatchRecord { namespace, .. }
        | WriteOp::DeleteRecord { namespace, .. }
        | WriteOp::TransferNamespace { namespace, .. }
        | WriteOp::Put { namespace, .. }
        | WriteOp::Delete { namespace, .. } => *namespace,
        WriteOp::CreateNamespace { .. } => unreachable!("namespace creation is checked by literal"),
    }
}

/// Predicts one typed record write: admission, then the charge walk with
/// the semantic checks at their charge index.
fn expect_write(pending: &Pending, path: Path, op: &WriteOp, budget: Budget) -> Predicted {
    let w = pending.weights;
    let record_op = matches!(
        op,
        WriteOp::CreateRecord { .. } | WriteOp::PatchRecord { .. } | WriteOp::DeleteRecord { .. }
    );
    if path == Path::Host && !record_op {
        return Predicted::NotAdmitted(Rejection::InvalidArgument(ArgumentFault::NotHostOperation));
    }
    if let Some(fault) = argument_fault(op) {
        return Predicted::NotAdmitted(Rejection::InvalidArgument(fault));
    }
    let limit = budget.limit_units().map(Cost::units);
    if let Some(limit) = limit
        && limit > pending.block_remaining
    {
        return Predicted::NotAdmitted(Rejection::ExceedsBlockBudget {
            remaining: Cost::new(pending.block_remaining),
        });
    }
    let limit = limit.unwrap_or(pending.block_remaining);
    assert!(record_op, "the workload issues record writes only");
    let namespace = op_namespace(op);
    let rejected = |reason: Rejection, charges: Vec<u64>, at: usize| {
        let (outcome, spent) = verdict(&charges, Some((at, reason)), limit);
        Predicted::Recorded {
            outcome,
            spent,
            applied: None,
        }
    };
    let Some(model) = pending.view.get(&namespace) else {
        return rejected(Rejection::UnknownNamespace, vec![w.directory_lookup], 0);
    };
    if let Path::Owner(actor) = path
        && actor != model.owner
    {
        return rejected(Rejection::Unauthorized, vec![w.directory_lookup], 0);
    }
    let lookup = vec![w.directory_lookup, w.point_read];
    let (charges, effect, mutation) = match op {
        WriteOp::CreateRecord { key, cells, .. } => {
            if model.bound(key).is_some() {
                return rejected(Rejection::AlreadyExists, lookup, 1);
            }
            let cells = cells_of(cells);
            let id = rid(model.next_id);
            (
                w.create_record_charges(model, key, &cells),
                Effect::RecordCreated { id },
                Mutation::Create {
                    key: key.clone(),
                    id,
                    cells,
                },
            )
        }
        WriteOp::PatchRecord {
            key, set, remove, ..
        } => {
            let Some((id, rec)) = model.bound(key) else {
                return rejected(Rejection::NotFound, lookup, 1);
            };
            if patched_cells(&rec.cells, set, remove).len() > LIMITS.max_cells {
                return rejected(Rejection::RecordFull, lookup, 1);
            }
            (
                w.patch_record_charges(model, id, key, &rec.cells, set, remove),
                Effect::RecordPatched { id },
                Mutation::Patch {
                    id,
                    set: set.clone(),
                    remove: remove.clone(),
                },
            )
        }
        WriteOp::DeleteRecord { key, .. } => {
            let Some((id, rec)) = model.bound(key) else {
                return rejected(Rejection::NotFound, lookup, 1);
            };
            (
                w.delete_record_charges(&rec.cells),
                Effect::RecordDeleted { id },
                Mutation::Delete { id },
            )
        }
        _ => unreachable!("record writes only"),
    };
    let (outcome, spent) = verdict(&charges, None, limit);
    Predicted::Recorded {
        outcome,
        spent,
        applied: (outcome == Outcome::Applied).then_some((effect, mutation)),
    }
}

/// The total an op would spend if it ran to completion (or to its semantic
/// rejection) under an unlimited budget; zero for an admission failure.
fn full_cost(pending: &Pending, path: Path, op: &WriteOp) -> u64 {
    match expect_write(pending, path, op, Budget::unlimited()) {
        Predicted::NotAdmitted(_) => 0,
        Predicted::Recorded { spent, .. } => spent,
    }
}

fn expected_flat(expected: &Predicted, schedule_id: u32) -> Flat<Effect> {
    match expected {
        Predicted::NotAdmitted(reason) => rejection(ReceiptKind::Write, schedule_id, 0, *reason),
        Predicted::Recorded {
            outcome: Outcome::Applied,
            spent,
            applied: Some((effect, _)),
        } => Ok((
            *effect,
            receipt(ReceiptKind::Write, schedule_id, *spent, Outcome::Applied),
        )),
        Predicted::Recorded {
            outcome: Outcome::Rejected(reason),
            spent,
            applied: None,
        } => rejection(ReceiptKind::Write, schedule_id, *spent, *reason),
        other => unreachable!("inconsistent expectation {other:?}"),
    }
}

/// The deletes an expiry run applied, in (height, id) order.
type Expired = Vec<(RecordId, Receipt)>;
/// A halted run: the deletes applied before the rejection, its reason and
/// its receipt.
type Halted = (Expired, Rejection, Receipt);

#[derive(Debug, PartialEq, Eq)]
enum Event {
    Published(SnapshotId),
    Write(Flat<Effect>),
    Rollback(Range<usize>),
    StaleHead(EngineError),
    Expiry(Result<Expired, Halted>),
    Read(Receipt),
    Page(Vec<RecordId>, Option<RecordId>),
}

#[derive(Debug, Default)]
struct Coverage {
    verdicts: BTreeMap<String, usize>,
    /// Applied patches that changed an attribute's type under the same name.
    type_changes: usize,
    /// Applied patches that switched a cell between attribute and field.
    kind_changes: usize,
    rollbacks: usize,
    released_ids: usize,
    recreated_keys: usize,
    expired: usize,
    same_block_expiries: usize,
    late_expiries: usize,
    multi_page_expiries: usize,
    halts: usize,
    field_survivors: usize,
    new_terms: usize,
    reused_terms: usize,
    deleted_later: usize,
    /// Applied record writes under the 32-byte key.
    longest_key_writes: usize,
}

/// The fixed query catalogue: every operator, both polarities, AND, OR,
/// match-all and the empty cases, under every projection shape.
fn catalogue() -> Vec<(&'static str, Query, Projection)> {
    let n = name;
    let s = Value::str;
    let named = Projection::named(false, [n(KIND), n(NOTE), n(GHOST)]);
    let one = |term: Term| Query::term(term);
    let and = |literals: Vec<Literal>| Query::all(literals);
    let or = |groups: Vec<Vec<Literal>>| Query { groups };
    vec![
        ("match-all", Query::match_all(), Projection::ALL),
        (
            "eq-kind-book",
            one(Term::eq(n(KIND), s("book"))),
            Projection::ID_ONLY,
        ),
        (
            "eq-price-10",
            one(Term::eq(n(PRICE), Value::I64(10))),
            Projection::KEY_ONLY,
        ),
        (
            "eq-price-u64-10",
            one(Term::eq(n(PRICE), Value::U64(10))),
            Projection::ALL,
        ),
        (
            "eq-flag-true",
            one(Term::eq(n(FLAG), Value::Bool(true))),
            named.clone(),
        ),
        (
            "noteq-kind-book",
            one(Term::not_eq(n(KIND), s("book"))),
            Projection::ALL,
        ),
        (
            "noteq-qty-7",
            one(Term::not_eq(n(QTY), Value::U64(7))),
            Projection::ID_ONLY,
        ),
        (
            "range-price-incl-incl",
            one(Term::range(
                n(PRICE),
                Bound::Included(Value::I64(0)),
                Bound::Included(Value::I64(20)),
            )),
            Projection::ID_ONLY,
        ),
        (
            "range-price-excl-unbounded",
            one(Term::range(
                n(PRICE),
                Bound::Excluded(Value::I64(-5)),
                Bound::Unbounded,
            )),
            Projection::ALL,
        ),
        (
            "range-price-unbounded-excl",
            one(Term::range(
                n(PRICE),
                Bound::Unbounded,
                Bound::Excluded(Value::I64(10)),
            )),
            Projection::KEY_ONLY,
        ),
        (
            "range-qty-inverted",
            one(Term::range(
                n(QTY),
                Bound::Included(Value::U64(300)),
                Bound::Excluded(Value::U64(1)),
            )),
            Projection::ID_ONLY,
        ),
        (
            "range-qty-excl-excl",
            one(Term::range(
                n(QTY),
                Bound::Excluded(Value::U64(0)),
                Bound::Excluded(Value::U64(300)),
            )),
            named.clone(),
        ),
        (
            "range-expires-le-40",
            one(Term::range(
                n(EXPIRES),
                Bound::Unbounded,
                Bound::Included(Value::U64(40)),
            )),
            Projection::ID_ONLY,
        ),
        (
            "starts-kind-b",
            one(Term::starts_with(n(KIND), "b")),
            Projection::ALL,
        ),
        (
            "starts-tag-empty",
            one(Term::starts_with(n(TAG), "")),
            Projection::ID_ONLY,
        ),
        (
            "starts-tag-ab",
            one(Term::starts_with(n(TAG), "ab")),
            Projection::KEY_ONLY,
        ),
        (
            "exists-price-i64",
            one(Term::exists(n(PRICE), TypeTag::I64)),
            Projection::ID_ONLY,
        ),
        (
            "exists-price-u64",
            one(Term::exists(n(PRICE), TypeTag::U64)),
            Projection::ID_ONLY,
        ),
        (
            "exists-note-str",
            one(Term::exists(n(NOTE), TypeTag::Str)),
            Projection::ID_ONLY,
        ),
        (
            "exists-flag-bool",
            one(Term::exists(n(FLAG), TypeTag::Bool)),
            Projection::ALL,
        ),
        (
            "not-eq-kind-book",
            and(vec![Literal::not(Term::eq(n(KIND), s("book")))]),
            Projection::ID_ONLY,
        ),
        (
            "not-exists-expires",
            and(vec![Literal::not(Term::exists(n(EXPIRES), TypeTag::U64))]),
            Projection::KEY_ONLY,
        ),
        (
            "and-eq-range",
            and(vec![
                Literal::of(Term::eq(n(KIND), s("book"))),
                Literal::of(Term::range(
                    n(PRICE),
                    Bound::Included(Value::I64(0)),
                    Bound::Unbounded,
                )),
            ]),
            Projection::ID_ONLY,
        ),
        (
            "and-range-not-eq",
            and(vec![
                Literal::of(Term::range(
                    n(QTY),
                    Bound::Included(Value::U64(1)),
                    Bound::Unbounded,
                )),
                Literal::not(Term::eq(n(FLAG), Value::Bool(true))),
            ]),
            Projection::ALL,
        ),
        (
            "and-noteq-starts",
            and(vec![
                Literal::of(Term::not_eq(n(KIND), s("pen"))),
                Literal::of(Term::starts_with(n(TAG), "a")),
            ]),
            Projection::ID_ONLY,
        ),
        (
            "and-not-not",
            and(vec![
                Literal::not(Term::exists(n(PRICE), TypeTag::I64)),
                Literal::not(Term::eq(n(KIND), s("cup"))),
            ]),
            Projection::ID_ONLY,
        ),
        (
            "and-exists-eq-eq",
            and(vec![
                Literal::of(Term::exists(n(QTY), TypeTag::U64)),
                Literal::of(Term::eq(n(KIND), s("book"))),
                Literal::of(Term::eq(n(FLAG), Value::Bool(false))),
            ]),
            named,
        ),
        (
            "or-book-pen",
            or(vec![
                vec![Literal::of(Term::eq(n(KIND), s("book")))],
                vec![Literal::of(Term::eq(n(KIND), s("pen")))],
            ]),
            Projection::ID_ONLY,
        ),
        (
            "or-range-matchall",
            or(vec![
                vec![Literal::of(Term::range(
                    n(PRICE),
                    Bound::Included(Value::I64(10)),
                    Bound::Unbounded,
                ))],
                vec![],
            ]),
            Projection::ALL,
        ),
        (
            "or-overlapping",
            or(vec![
                vec![Literal::of(Term::eq(n(KIND), s("book")))],
                vec![Literal::of(Term::exists(n(PRICE), TypeTag::I64))],
                vec![Literal::not(Term::eq(n(FLAG), Value::Bool(true)))],
            ]),
            Projection::KEY_ONLY,
        ),
        (
            "or-both-empty",
            or(vec![
                vec![Literal::of(Term::eq(n(KIND), s("zzz")))],
                vec![Literal::of(Term::range(
                    n(QTY),
                    Bound::Included(Value::U64(300)),
                    Bound::Excluded(Value::U64(1)),
                ))],
            ]),
            Projection::ID_ONLY,
        ),
    ]
}

/// Everything a boundary or history check produces, for comparison.
#[derive(Debug, Default, PartialEq, Eq)]
struct Checked {
    receipts: Vec<Receipt>,
    pages: Vec<(Vec<RecordId>, Option<RecordId>)>,
}

/// A cursor issued during a check plus what it was issued for.
struct Issued {
    cursor: Cursor,
    query: Query,
    projection: Projection,
}

impl Transcript<Event> {
    fn record_reads(&mut self, checked: &Checked) {
        self.events
            .extend(checked.receipts.iter().cloned().map(Event::Read));
    }

    fn record_pages(&mut self, checked: &Checked) {
        let pages = checked.pages.iter().cloned();
        self.events
            .extend(pages.map(|(ids, next)| Event::Page(ids, next)));
    }
}

/// The receipt of a read that ran to completion.
fn completed_read(weights: &Weights, spent: u64) -> Receipt {
    receipt(ReceiptKind::Read, weights.id, spent, Outcome::Applied)
}

/// Any read of a namespace that does not exist: rejected after the lookup.
fn unknown_namespace<T>(weights: &Weights) -> Flat<T> {
    rejection(
        ReceiptKind::Read,
        weights.id,
        weights.directory_lookup,
        Rejection::UnknownNamespace,
    )
}

/// A cursor presented where it was not issued: rejected at admission.
fn cursor_mismatch<T>(weights: &Weights) -> Flat<T> {
    rejection(ReceiptKind::Read, weights.id, 0, Rejection::CursorMismatch)
}

/// What `get_record` returns for `key`: the projected row when the key is
/// bound.
fn expected_get_record(
    model: Option<&NsModel>,
    key: &RecordKey,
    projection: &Projection,
    weights: &Weights,
) -> Flat<Option<Row>> {
    let Some(model) = model else {
        return unknown_namespace(weights);
    };
    let row = model
        .bound(key)
        .map(|(id, rec)| project(id, rec, projection));
    let charges = weights.get_record_charges(row.as_ref().map(row_bytes));
    expected_read(row, &charges, None, u64::MAX, weights.id)
}

/// The expected page after `after`: the reference matches chunked by
/// `limit`, with `next` exactly when a further match exists.
fn expected_page(
    model: &NsModel,
    all: &[RecordId],
    projection: &Projection,
    limit: usize,
    after: Option<RecordId>,
) -> (Vec<Row>, Option<RecordId>) {
    let rest: Vec<RecordId> = all
        .iter()
        .copied()
        .filter(|id| after.is_none_or(|a| *id > a))
        .collect();
    let rows: Vec<Row> = rest
        .iter()
        .take(limit)
        .map(|id| project(*id, &model.rows[id], projection))
        .collect();
    let next = (rest.len() > limit).then(|| rows[limit - 1].id);
    (rows, next)
}

/// One query on one reader, with what every page of its walk is checked
/// against.
struct PageWalk<'a> {
    reader: &'a dyn Reader,
    model: &'a NsModel,
    weights: &'a Weights,
    namespace: NamespaceId,
    label: &'a str,
    query: &'a Query,
    projection: &'a Projection,
    /// Every match of the reference evaluator, in id order.
    all: &'a [RecordId],
    ctx: &'a Ctx,
}

impl PageWalk<'_> {
    /// Checks one page against the reference and, for a single group,
    /// against the predicted charges; returns the page's cursor.
    fn check_page(
        &self,
        limit: usize,
        cursor: Option<&Cursor>,
        probe_budget: bool,
        out: &mut Checked,
    ) -> Option<Cursor> {
        let Self {
            reader,
            model,
            weights,
            namespace,
            label,
            query,
            projection,
            all,
            ctx,
        } = *self;
        let after = cursor.map(Cursor::after);
        let (rows, next) = expected_page(model, all, projection, limit, after);
        let where_ = format!("{ctx} ns={namespace} query={label} after={after:?} limit={limit}");
        let page = reader
            .query(
                namespace,
                query,
                projection,
                limit,
                cursor,
                Budget::unlimited(),
            )
            .unwrap_or_else(|rejected| panic!("{where_}: rejected: {rejected}"));
        assert_eq!(page.value.rows, rows, "{where_}: rows");
        assert_eq!(
            page.value.next.as_ref().map(Cursor::after),
            next,
            "{where_}: continuation"
        );
        if let Some(issued) = &page.value.next {
            assert_eq!(
                issued.snapshot(),
                reader.pinned(),
                "{where_}: cursor context"
            );
            assert_eq!(issued.namespace(), namespace, "{where_}: cursor namespace");
            assert_eq!(issued.query(), query, "{where_}: cursor query");
            assert_eq!(
                issued.projection(),
                projection,
                "{where_}: cursor projection"
            );
            // The same cursor under another projection is a mismatch at
            // admission: spent 0, receipt issued.
            let other = if *projection == Projection::ID_ONLY {
                Projection::KEY_ONLY
            } else {
                Projection::ID_ONLY
            };
            let mismatch = reader.query(
                namespace,
                query,
                &other,
                limit,
                Some(issued),
                Budget::unlimited(),
            );
            assert_eq!(
                flat_read(&mismatch).0,
                cursor_mismatch(weights),
                "{where_}: cursor bound to the projection"
            );
        }
        if let Some(group) = single_group(query) {
            let plan = predict_page(weights, model, group, projection, limit, after);
            let ids: Vec<RecordId> = rows.iter().map(|row| row.id).collect();
            assert_eq!(
                plan.rows, ids,
                "{where_}: predictor disagrees with the evaluator"
            );
            assert_eq!(plan.next, next, "{where_}: predictor continuation");
            let full = total(&plan.charges);
            assert_eq!(
                page.receipt,
                completed_read(weights, full),
                "{where_}: page receipt, charges {:?}",
                plan.charges
            );
            if probe_budget && full > 0 {
                // One unit short stops at the last charge point that fits, with
                // no partial page.
                let short = reader.query(
                    namespace,
                    query,
                    projection,
                    limit,
                    cursor,
                    Budget::limit(full - 1),
                );
                let (outcome, spent) = verdict(&plan.charges, None, full - 1);
                assert_eq!(
                    outcome,
                    Outcome::Rejected(Rejection::OutOfBudget),
                    "{where_}"
                );
                assert_eq!(
                    flat_read(&short).0,
                    rejection(ReceiptKind::Read, weights.id, spent, Rejection::OutOfBudget),
                    "{where_}: one unit short of {full}, charges {:?}",
                    plan.charges
                );
            }
        }
        out.receipts.push(page.receipt.clone());
        out.pages
            .push((rows.iter().map(|row| row.id).collect(), next));
        page.value.next
    }
}

/// Checks statistics, `get_record` for every key, and `count` plus a full
/// cursor walk of the catalogue for every namespace of `view` and the
/// sentinel. Returns the receipts and pages in order plus one cursor.
fn check_reads(
    reader: &impl Reader,
    view: &View,
    weights: &Weights,
    ctx: &Ctx,
) -> (Checked, Option<Issued>) {
    let mut out = Checked::default();
    let mut issued: Option<Issued> = None;
    let named = Projection::named(true, [name(KIND), name(NOTE), name(GHOST)]);
    let catalogue = catalogue();
    for namespace in [ns(NS1), ns(NS2), ns(SENTINEL)] {
        let model = view.get(&namespace);
        assert_eq!(
            reader.typed_record_count(namespace),
            model.map(|m| m.rows.len()),
            "{ctx} ns={namespace}: typed record count"
        );
        assert_eq!(
            reader.term_count(namespace),
            model.map(NsModel::term_count),
            "{ctx} ns={namespace}: term count"
        );
        assert_eq!(
            reader.index_consistent(namespace),
            model.map(|_| true),
            "{ctx} ns={namespace}: index consistency"
        );

        for bytes in KEYS.iter().copied().chain([ABSENT_KEY]) {
            let record_key = key(bytes);
            for projection in [&Projection::ALL, &named] {
                let want = expected_get_record(model, &record_key, projection, weights);
                let got = flat_read(&reader.get_record(
                    namespace,
                    &record_key,
                    projection,
                    Budget::unlimited(),
                ))
                .0;
                assert_eq!(got, want, "{ctx} ns={namespace} get_record key={bytes:?}");
                out.receipts.push(receipt_of(&got));
            }
        }

        for (label, query, projection) in &catalogue {
            let Some(model) = model else {
                let count = flat_read(&reader.count(namespace, query, Budget::unlimited())).0;
                assert_eq!(
                    count,
                    unknown_namespace(weights),
                    "{ctx} ns={namespace} count {label}"
                );
                out.receipts.push(receipt_of(&count));
                let page = flat_read(&reader.query(
                    namespace,
                    query,
                    projection,
                    PAGE,
                    None,
                    Budget::unlimited(),
                ))
                .0;
                assert_eq!(
                    page,
                    unknown_namespace(weights),
                    "{ctx} ns={namespace} query {label}"
                );
                out.receipts.push(receipt_of(&page));
                continue;
            };
            let all = model.matches(query);
            let counted = reader
                .count(namespace, query, Budget::unlimited())
                .unwrap_or_else(|rejected| {
                    panic!("{ctx} ns={namespace} count {label}: {rejected}")
                });
            assert_eq!(
                counted.value,
                count(all.len()),
                "{ctx} ns={namespace} count {label}"
            );
            if let Some(group) = single_group(query) {
                let plan = predict_count(weights, model, group);
                assert_eq!(
                    plan.rows, all,
                    "{ctx} {label}: count predictor vs evaluator"
                );
                assert_eq!(
                    counted.receipt,
                    completed_read(weights, total(&plan.charges)),
                    "{ctx} ns={namespace} count {label}: charges {:?}",
                    plan.charges
                );
            }
            out.receipts.push(counted.receipt.clone());

            let walk = PageWalk {
                reader,
                model,
                weights,
                namespace,
                label,
                query,
                projection,
                all: &all,
                ctx,
            };
            let mut cursor: Option<Cursor> = None;
            let mut first = true;
            loop {
                let next = walk.check_page(PAGE, cursor.as_ref(), first, &mut out);
                if first
                    && *query == Query::match_all()
                    && let Some(next) = &next
                {
                    // The limit is not bound by the cursor: a smaller page
                    // continues from the same position.
                    walk.check_page(2, Some(next), true, &mut out);
                    if issued.is_none() {
                        issued = Some(Issued {
                            cursor: next.clone(),
                            query: query.clone(),
                            projection: projection.clone(),
                        });
                    }
                }
                first = false;
                match next {
                    Some(next) => cursor = Some(next),
                    None => break,
                }
            }
        }
    }
    (out, issued)
}

/// The workload's cell and patch draws.
impl Generator {
    fn kind_cell(&mut self) -> Cell {
        match self.index(8) {
            0 => field(Value::str(self.pick(&KINDS))),
            1 => attr(Value::Bool(true)),
            _ => attr(Value::str(self.pick(&KINDS))),
        }
    }

    fn price_cell(&mut self) -> Cell {
        let price = self.pick(&PRICES);
        match self.index(6) {
            0 => field(Value::I64(price)),
            1 => attr(Value::U64(price.unsigned_abs())),
            _ => attr(Value::I64(price)),
        }
    }

    fn qty_cell(&mut self) -> Cell {
        attr(Value::U64(self.pick(&QTYS)))
    }

    fn flag_cell(&mut self) -> Cell {
        attr(Value::Bool(self.chance(2)))
    }

    fn tag_cell(&mut self) -> Cell {
        if self.chance(6) {
            field(Value::str(self.pick(&TAGS)))
        } else {
            attr(Value::str(self.pick(&TAGS)))
        }
    }

    fn note_cell(&mut self) -> Cell {
        field(Value::bytes(self.pick(&NOTES)))
    }

    /// A cell for `which` in 0..5 (price, qty, flag, tag, note).
    fn optional_cell(&mut self, which: usize) -> (CellName, Cell) {
        match which {
            0 => (name(PRICE), self.price_cell()),
            1 => (name(QTY), self.qty_cell()),
            2 => (name(FLAG), self.flag_cell()),
            3 => (name(TAG), self.tag_cell()),
            _ => (name(NOTE), self.note_cell()),
        }
    }

    /// `$expiresAt` at creation: a future block, or now and then this block.
    fn expiry_cell(&mut self, block: u64) -> Cell {
        if self.chance(8) {
            attr(Value::U64(block))
        } else {
            attr(Value::U64(block + 1 + count(self.index(6))))
        }
    }

    /// Creation cells in a shuffled order: `$expiresAt`, usually `kind`,
    /// and a subset of the optional names, at most `max_cells` in total.
    fn create_cells(&mut self, block: u64) -> Cells {
        let mut cells = vec![(name(EXPIRES), self.expiry_cell(block))];
        if !self.chance(6) {
            cells.push((name(KIND), self.kind_cell()));
        }
        let mut optional: Vec<usize> = (0..5).filter(|_| self.chance(2)).collect();
        while cells.len() + optional.len() > LIMITS.max_cells {
            optional.pop();
        }
        for which in optional {
            cells.push(self.optional_cell(which));
        }
        let pivot = self.index(cells.len());
        cells.rotate_left(pivot);
        cells
    }

    /// A patch: set some names (value, type and kind changes) and remove
    /// some (present or absent), never overlapping.
    fn patch_lists(&mut self) -> (Cells, Vec<CellName>) {
        let mut set: Cells = Vec::new();
        let mut used: BTreeSet<usize> = BTreeSet::new();
        for _ in 0..self.index(3) {
            let which = self.index(6);
            if used.insert(which) {
                set.push(match which {
                    5 => (name(KIND), self.kind_cell()),
                    other => self.optional_cell(other),
                });
            }
        }
        let mut remove: Vec<CellName> = Vec::new();
        for _ in 0..self.index(3) {
            let which = self.index(7);
            if used.insert(which) {
                remove.push(match which {
                    5 => name(KIND),
                    6 => name(GHOST),
                    other => [PRICE, QTY, FLAG, TAG, NOTE].map(name)[other].clone(),
                });
            }
        }
        if set.is_empty() && remove.is_empty() {
            set.push((name(FLAG), self.flag_cell()));
        }
        (set, remove)
    }

    /// A patch of `$expiresAt`: expire now, in the past, later, become a
    /// field or another type, or go immortal.
    fn expiry_patch(&mut self, block: u64) -> (Cells, Vec<CellName>) {
        let expires = name(EXPIRES);
        match self.index(6) {
            0 => (vec![(expires, attr(Value::U64(block)))], vec![]),
            1 => (
                vec![(expires, attr(Value::U64(block.saturating_sub(3).max(1))))],
                vec![],
            ),
            2 => (vec![(expires, attr(Value::U64(block + 2)))], vec![]),
            3 => (vec![(expires, field(Value::U64(block)))], vec![]),
            4 => (vec![(expires, attr(Value::I64(0)))], vec![]),
            _ => (vec![], vec![expires]),
        }
    }
}

/// An operation that fails admission, chosen by `which` (case 4, the empty
/// patch, is a no-op or NotFound instead).
fn fault_op(namespace: NamespaceId, bytes: &[u8], which: usize) -> (Path, WriteOp) {
    let long = "x".repeat(LIMITS.max_str_len + 1);
    let kind = name(KIND);
    let all_names = [KIND, PRICE, QTY, FLAG, TAG, NOTE, GHOST].map(name);
    let op = match which {
        // DuplicateCell.
        0 => create_record(
            namespace,
            bytes,
            vec![
                (kind.clone(), attr(Value::str("book"))),
                (kind, attr(Value::str("pen"))),
            ],
        ),
        // ValueTooLarge (a field is validated too).
        1 => create_record(
            namespace,
            bytes,
            vec![(name(TAG), field(Value::str(&long)))],
        ),
        // BytesNotIndexable.
        2 => create_record(
            namespace,
            bytes,
            vec![(name(NOTE), attr(Value::bytes(b"xyz")))],
        ),
        // TooManyCells: seven cells.
        3 => create_record(
            namespace,
            bytes,
            all_names
                .iter()
                .map(|n| (n.clone(), field(Value::Bool(true))))
                .collect(),
        ),
        // Empty patch: golem-db's no-op (or NotFound).
        4 => patch_record(namespace, bytes, vec![], vec![]),
        // TooManyCells: set + remove name seven cells.
        5 => patch_record(
            namespace,
            bytes,
            all_names[..4]
                .iter()
                .map(|n| (n.clone(), attr(Value::Bool(false))))
                .collect(),
            all_names[4..].to_vec(),
        ),
        // DuplicateCell in the remove list.
        6 => patch_record(namespace, bytes, vec![], vec![name(TAG), name(TAG)]),
        // SetAndRemoveOverlap.
        7 => patch_record(
            namespace,
            bytes,
            vec![(name(TAG), attr(Value::str("aa")))],
            vec![name(TAG)],
        ),
        // Per-cell order: ValueTooLarge on cell 0 wins over the duplicate.
        8 => create_record(
            namespace,
            bytes,
            vec![
                (name(TAG), attr(Value::str(&long))),
                (name(TAG), attr(Value::str("aa"))),
            ],
        ),
        // Per-cell order: BytesNotIndexable on cell 0 wins over the duplicate.
        9 => create_record(
            namespace,
            bytes,
            vec![
                (name(NOTE), attr(Value::bytes(b""))),
                (name(NOTE), field(Value::bytes(b""))),
            ],
        ),
        // NotHostOperation: a raw byte write on the host path.
        10 => {
            return (Path::Host, put(namespace, bytes, b"raw"));
        }
        _ => {
            return (Path::Host, create_namespace("test"));
        }
    };
    (Path::Owner(ALICE), op)
}

/// Block write budgets: unlimited every fifth block, tight every third,
/// otherwise roomy. Tight blocks run dry and exercise admission, and may
/// halt expiry.
fn block_budget(block: u64, size: usize) -> Option<u64> {
    let size = count(size);
    if block.is_multiple_of(5) || is_stamping_block(block) {
        None
    } else if block % 3 == 1 {
        Some(size * 40 + 90)
    } else {
        Some(size * 220 + 600)
    }
}

/// Every 20th block stamps every one of its keys to expire in the next
/// block (a starved one), so expiry runs over several pages; every
/// fourth stamp is a field, which expiry must leave alone.
fn is_stamping_block(block: u64) -> bool {
    block % 20 == 19
}

fn stamp_op(
    generator: &mut Generator,
    block: u64,
    index: usize,
    pending: &Pending,
) -> (Path, WriteOp, Budget) {
    let namespace = ns(if index.is_multiple_of(2) { NS1 } else { NS2 });
    let bytes = KEYS[index % KEYS.len()];
    let model = &pending.view[&namespace];
    let stamp = if index % 4 == 3 {
        field(Value::U64(block + 1))
    } else {
        attr(Value::U64(block + 1))
    };
    let op = if model.bound(&key(bytes)).is_some() {
        patch_record(namespace, bytes, vec![(name(EXPIRES), stamp)], vec![])
    } else {
        let mut cells = generator.create_cells(block);
        for (cell_name, cell) in &mut cells {
            if cell_name.as_str() == EXPIRES {
                *cell = stamp.clone();
            }
        }
        create_record(namespace, bytes, cells)
    };
    (Path::Owner(model.owner), op, Budget::unlimited())
}

fn plan_op(
    generator: &mut Generator,
    block: u64,
    index: usize,
    focus: &[u8],
    pending: &Pending,
) -> (Path, WriteOp, Budget) {
    if is_stamping_block(block) {
        return stamp_op(generator, block, index, pending);
    }
    let namespace = if generator.chance(12) {
        ns(SENTINEL)
    } else {
        ns(generator.pick(&[NS1, NS2]))
    };
    let owner = pending.view.get(&namespace).map(|model| model.owner);
    let path = if generator.chance(6) {
        Path::Host
    } else {
        match owner {
            Some(owner) if !generator.chance(5) => Path::Owner(owner),
            _ => Path::Owner(generator.pick(&OWNERS)),
        }
    };
    // Half of the time a live key of the namespace, so patches, deletes and
    // duplicate creations meet a bound key.
    let live: Vec<&[u8]> = pending
        .view
        .get(&namespace)
        .map(|model| model.keys.keys().map(RecordKey::as_bytes).collect())
        .unwrap_or_default();
    let bytes = if matches!(index % 10, 2 | 3) {
        focus
    } else if !live.is_empty() && generator.chance(2) {
        generator.pick(&live)
    } else {
        generator.pick(&KEYS)
    };
    let (path, op) = match index % 10 {
        // 2 then 3 delete and recreate the focus key: the recreation mints a
        // new id.
        0 | 1 | 3 => (
            path,
            create_record(namespace, bytes, generator.create_cells(block)),
        ),
        2 | 6 => (path, delete_record(namespace, bytes)),
        4 | 5 => {
            let (set, remove) = generator.patch_lists();
            (path, patch_record(namespace, bytes, set, remove))
        }
        7 => {
            let (set, remove) = generator.expiry_patch(block);
            (path, patch_record(namespace, bytes, set, remove))
        }
        8 => fault_op(namespace, bytes, generator.index(12)),
        // Likely RecordFull: three names the record may not hold yet.
        _ => (
            path,
            patch_record(
                namespace,
                bytes,
                vec![
                    (name(FLAG), attr(Value::Bool(true))),
                    (name(TAG), attr(Value::str("b"))),
                    (name(QTY), attr(Value::U64(7))),
                ],
                vec![],
            ),
        ),
    };
    let cost = full_cost(pending, path, &op);
    (path, op, draw_budget(generator, cost))
}

fn run_write(branch: &mut Branch, path: Path, op: WriteOp, budget: Budget) -> WriteResult {
    match path {
        Path::Owner(actor) => branch.execute(actor, op, budget),
        Path::Host => branch.execute_host(op, budget),
    }
}

fn run_estimate(branch: &Branch, path: Path, op: WriteOp, budget: Budget) -> WriteResult {
    match path {
        Path::Owner(actor) => branch.estimate(actor, op, budget),
        Path::Host => branch.estimate_host(op, budget),
    }
}

#[allow(clippy::too_many_arguments)]
fn execute_checked(
    branch: &mut Branch,
    pending: &mut Pending,
    path: Path,
    op: WriteOp,
    budget: Budget,
    thorough: bool,
    ctx: &Ctx,
    transcript: &mut Transcript<Event>,
    coverage: &mut Coverage,
) {
    let expected = expect_write(pending, path, &op, budget);
    let want = expected_flat(&expected, pending.weights.id);
    if thorough {
        let estimated = flat_write(&run_estimate(branch, path, op.clone(), budget)).0;
        assert_eq!(estimated, want, "{ctx} estimate path={path:?} op={op:?}");
    }
    let receipts_before = branch.receipts().len();
    let result = run_write(branch, path, op.clone(), budget);
    let (got, physical) = flat_write(&result);
    assert_eq!(
        got, want,
        "{ctx} path={path:?} op={op:?} budget={budget:?} block_remaining={}",
        pending.block_remaining
    );
    match expected {
        Predicted::NotAdmitted(_) => {
            assert_eq!(
                branch.receipts().len(),
                receipts_before,
                "{ctx}: admission rejections are never recorded"
            );
        }
        Predicted::Recorded {
            outcome,
            spent,
            applied,
        } => {
            pending.record(outcome, spent);
            if let Some((_, mutation)) = applied {
                let namespace = op_namespace(&op);
                if let WriteOp::CreateRecord { key: k, .. }
                | WriteOp::PatchRecord { key: k, .. }
                | WriteOp::DeleteRecord { key: k, .. } = &op
                    && k.as_bytes().len() == 32
                {
                    coverage.longest_key_writes += 1;
                }
                if let Mutation::Create { key, .. } = &mutation
                    && pending
                        .mutations
                        .iter()
                        .any(|(n, m)| *n == namespace && matches!(m, Mutation::Delete { .. }))
                    && !pending.view[&namespace].keys.contains_key(key)
                {
                    coverage.recreated_keys += 1;
                }
                if let Mutation::Create { cells, .. } = &mutation {
                    let model = &pending.view[&namespace];
                    for (n, cell) in cells {
                        if cell.is_attribute() {
                            if model.holds_value(n, &cell.value, None) {
                                coverage.reused_terms += 1;
                            } else {
                                coverage.new_terms += 1;
                            }
                        }
                    }
                }
                if let Mutation::Patch { id, set, .. } = &mutation {
                    let before = &pending.view[&namespace].rows[id].cells;
                    for (n, cell) in set {
                        if let Some(old) = before.get(n) {
                            if old.kind != cell.kind {
                                coverage.kind_changes += 1;
                            }
                            if old.is_attribute()
                                && cell.is_attribute()
                                && old.value.tag() != cell.value.tag()
                            {
                                coverage.type_changes += 1;
                            }
                        }
                    }
                }
                pending.mutate(namespace, mutation);
            }
        }
    }
    assert_eq!(
        branch.spent(),
        Cost::new(pending.spent()),
        "{ctx} after {op:?}"
    );
    assert_eq!(
        branch.remaining(),
        Cost::new(pending.block_remaining),
        "{ctx} after {op:?}"
    );
    assert_eq!(
        branch.receipts(),
        pending.receipts.as_slice(),
        "{ctx} after {op:?}: recorded receipts"
    );
    if thorough {
        for namespace in [ns(NS1), ns(NS2)] {
            let model = pending.view.get(&namespace);
            assert_eq!(
                branch.index_consistent(namespace),
                model.map(|_| true),
                "{ctx} after {op:?}: index consistency"
            );
            assert_eq!(
                branch.typed_record_count(namespace),
                model.map(|m| m.rows.len()),
                "{ctx} after {op:?}: record count"
            );
        }
        if let WriteOp::CreateRecord { key: k, .. }
        | WriteOp::PatchRecord { key: k, .. }
        | WriteOp::DeleteRecord { key: k, .. } = &op
        {
            let namespace = op_namespace(&op);
            let model = pending.view.get(&namespace);
            let want = expected_get_record(model, k, &Projection::ALL, &pending.weights);
            let got =
                flat_read(&branch.get_record(namespace, k, &Projection::ALL, Budget::unlimited()))
                    .0;
            assert_eq!(got, want, "{ctx} after {op:?}: branch get_record");
        }
    }
    *coverage.verdicts.entry(verdict_name(&got)).or_default() += 1;
    transcript.events.push(Event::Write(got));
    transcript.physical.push(physical);
}

fn rollback_checked(
    branch: &mut Branch,
    pending: &mut Pending,
    checkpoints: &[Checkpoint],
    index: usize,
    ctx: &Ctx,
    transcript: &mut Transcript<Event>,
    coverage: &mut Coverage,
) {
    let (discarded, released) = pending.rollback(index);
    let got = branch.rollback_to(checkpoints[index]);
    assert_eq!(
        got,
        Ok(discarded.clone()),
        "{ctx} rollback to checkpoint {index}: discarded receipt range"
    );
    assert_eq!(
        branch.receipts(),
        pending.receipts.as_slice(),
        "{ctx} rollback to checkpoint {index}: receipts are never withdrawn"
    );
    assert_eq!(branch.spent(), Cost::new(pending.spent()), "{ctx} rollback");
    for namespace in [ns(NS1), ns(NS2)] {
        let model = pending.view.get(&namespace);
        assert_eq!(
            branch.typed_record_count(namespace),
            model.map(|m| m.rows.len()),
            "{ctx} rollback to checkpoint {index}: rows restored"
        );
        assert_eq!(
            branch.term_count(namespace),
            model.map(NsModel::term_count),
            "{ctx} rollback to checkpoint {index}: terms restored"
        );
        assert_eq!(
            branch.index_consistent(namespace),
            model.map(|_| true),
            "{ctx} rollback to checkpoint {index}: index consistency"
        );
    }
    coverage.rollbacks += 1;
    coverage.released_ids += released;
    transcript.events.push(Event::Rollback(discarded));
}

/// Runs the host's expiry step for `namespace` at `block` and checks it
/// against the oracle: the expiring rows height by height and in id order
/// within a height, page by page, each delete receipt, and the halt when a
/// delete is rejected. Returns the ids that expired.
#[allow(clippy::too_many_arguments)]
fn expire_checked(
    branch: &mut Branch,
    pending: &mut Pending,
    namespace: NamespaceId,
    block: u64,
    starved: bool,
    ctx: &Ctx,
    transcript: &mut Transcript<Event>,
    coverage: &mut Coverage,
) -> Vec<RecordId> {
    let expires = name(EXPIRES);
    let query = Query::term(Term::range(
        expires.clone(),
        Bound::Unbounded,
        Bound::Included(Value::U64(block)),
    ));
    let delete_budget = if starved {
        Budget::limit(pending.weights.directory_lookup)
    } else {
        Budget::unlimited()
    };
    // Rows whose `$expiresAt` is a field, another type or absent survive
    // whatever their value: the engine assigns `$`-names no meaning.
    let survivors = pending.view[&namespace]
        .rows
        .values()
        .filter(|rec| {
            matches!(
                rec.cells.get(&expires),
                Some(Cell { kind: arkiv_db::CellKind::Field, value: Value::U64(at) }) if *at <= block
            )
        })
        .count();
    let mut applied: Vec<(RecordId, Receipt)> = Vec::new();
    let mut halt: Option<(Rejection, Receipt)> = None;
    let mut pages = 0usize;
    // The host sweeps height by height: the smallest live expiry height
    // first, its rows in id order in pages of PAGE, then the next height.
    'run: loop {
        let page: Vec<(RecordId, RecordKey)> = {
            let model = &pending.view[&namespace];
            let due = model.matches(&query);
            let Some(smallest) = due
                .iter()
                .filter_map(|id| match attribute(&model.rows[id], &expires) {
                    Some(Value::U64(at)) => Some(*at),
                    _ => None,
                })
                .min()
            else {
                break;
            };
            due.into_iter()
                .filter(|id| attribute(&model.rows[id], &expires) == Some(&Value::U64(smallest)))
                .take(PAGE)
                .map(|id| (id, model.rows[&id].key.clone()))
                .collect()
        };
        if page.is_empty() {
            break;
        }
        pages += 1;
        for (id, record_key) in page {
            let op = WriteOp::DeleteRecord {
                namespace,
                key: record_key,
            };
            match expect_write(pending, Path::Host, &op, delete_budget) {
                Predicted::Recorded {
                    outcome: Outcome::Applied,
                    spent,
                    applied: Some((effect, mutation)),
                } => {
                    assert_eq!(effect, Effect::RecordDeleted { id }, "{ctx}");
                    let created_this_block = pending.mutations.iter().any(|(n, m)| {
                        *n == namespace && matches!(m, Mutation::Create { id: c, .. } if *c == id)
                    });
                    let stamped = match pending.view[&namespace].rows[&id].cells.get(&expires) {
                        Some(cell) => match cell.value {
                            Value::U64(at) => at,
                            _ => unreachable!("only U64 attributes match the expiry range"),
                        },
                        None => unreachable!("expiring rows hold the attribute"),
                    };
                    if stamped < block {
                        coverage.late_expiries += 1;
                    }
                    if created_this_block {
                        coverage.same_block_expiries += 1;
                    }
                    let receipt = pending.record(Outcome::Applied, spent);
                    pending.mutate(namespace, mutation);
                    applied.push((id, receipt));
                }
                Predicted::Recorded {
                    outcome: Outcome::Rejected(reason),
                    spent,
                    ..
                } => {
                    halt = Some((reason, pending.record(Outcome::Rejected(reason), spent)));
                    break 'run;
                }
                Predicted::NotAdmitted(reason) => {
                    halt = Some((
                        reason,
                        receipt(
                            ReceiptKind::Write,
                            pending.weights.id,
                            0,
                            Outcome::Rejected(reason),
                        ),
                    ));
                    break 'run;
                }
                other => unreachable!("{ctx}: inconsistent expectation {other:?}"),
            }
        }
    }
    let got = expire_records(
        branch,
        namespace,
        &expires,
        BlockNumber(block),
        Budget::unlimited(),
        delete_budget,
    );
    let where_ = format!("{ctx} ns={namespace} expiry starved={starved}");
    let event = match (got, halt) {
        (Ok(list), None) => {
            assert_eq!(list, applied, "{where_}: expired rows and receipts");
            Ok(list)
        }
        (Err(halted), Some((reason, receipt))) => {
            assert_eq!(
                halted.applied, applied,
                "{where_}: deletes applied before the halt"
            );
            assert_eq!(halted.rejected.reason, reason, "{where_}: halt reason");
            assert_eq!(halted.rejected.receipt, receipt, "{where_}: halt receipt");
            coverage.halts += 1;
            Err((halted.applied, reason, receipt))
        }
        (Ok(list), Some(halt)) => panic!("{where_}: expected a halt {halt:?}, got {list:?}"),
        (Err(halted), None) => panic!("{where_}: unexpected halt: {halted}"),
    };
    assert_eq!(
        branch.receipts(),
        pending.receipts.as_slice(),
        "{where_}: receipts"
    );
    assert_eq!(branch.spent(), Cost::new(pending.spent()), "{where_}");
    assert_eq!(
        branch.index_consistent(namespace),
        Some(true),
        "{where_}: index consistency after expiry"
    );
    assert_eq!(
        branch.typed_record_count(namespace),
        Some(pending.view[&namespace].rows.len()),
        "{where_}: rows after expiry"
    );
    coverage.expired += applied.len();
    coverage.field_survivors += survivors;
    if pages > 1 && event.is_ok() {
        coverage.multi_page_expiries += 1;
    }
    let ids = applied.iter().map(|(id, _)| *id).collect();
    transcript.events.push(Event::Expiry(event));
    ids
}

/// Publishes a pricing change as its own snapshot before the block's data
/// candidate. The block maps to the data snapshot only.
fn publish_admin(
    engine: &mut Engine,
    oracle: &mut Oracle,
    block: u64,
    next_snapshot: &mut u64,
    transcript: &mut Transcript<Event>,
    seed: u32,
) {
    let ctx = Ctx::new(seed, block, "admin");
    let n = u32::try_from(block / ADMIN_EVERY).expect("small");
    // Distinct, increasing ids: 2, 3, ... after the fixture's 1.
    let id = n + 1;
    let current = schedule_at(&oracle.log.schedules, block).id;
    let (schedule, weights) = admin_schedule(n, id);
    let head = publish_pricing_candidate(engine, current, &schedule, next_snapshot, &ctx);
    transcript.events.push(Event::Published(head.id()));
    oracle.log.schedules.push((block, weights));
    // The admin snapshot carries the previous block's data, priced with the
    // new schedule.
    let (checked, _) = check_reads(&head, &oracle.head, &weights, &ctx);
    transcript.record_reads(&checked);
}

/// Builds a competing candidate on the current head: a creation that takes
/// the same provisional id as the block's, a host patch, branch cursors
/// (kept valid by a rejected write, invalidated by a rollback), and a host
/// attempt at a namespace operation. It loses once the real block publishes.
#[allow(clippy::too_many_arguments)]
fn build_loser(
    engine: &Engine,
    oracle: &Oracle,
    generator: &mut Generator,
    weights: Weights,
    seed: u32,
    block: u64,
    transcript: &mut Transcript<Event>,
    coverage: &mut Coverage,
) -> SealedCandidate {
    let ctx = Ctx::new(seed, block, "loser");
    let mut branch = engine.begin(engine.head(), None).unwrap();
    let mut pending = Pending::open(oracle, weights, None);
    let bytes = generator.pick(&KEYS);
    let namespace = ns(NS1);
    let expected_id = rid(pending.view[&namespace].next_id);
    let steps = [
        (
            Path::Owner(ALICE),
            create_record(namespace, bytes, generator.create_cells(block)),
            Budget::unlimited(),
        ),
        (
            Path::Host,
            patch_record(
                namespace,
                bytes,
                vec![(name(KIND), attr(Value::str("cup")))],
                vec![name(GHOST)],
            ),
            Budget::unlimited(),
        ),
    ];
    let bound_before = pending.view[&namespace].bound(&key(bytes)).is_some();
    for (path, op, budget) in steps {
        execute_checked(
            &mut branch,
            &mut pending,
            path,
            op,
            budget,
            true,
            &ctx,
            transcript,
            coverage,
        );
    }
    if !bound_before {
        assert_eq!(
            pending.view[&namespace]
                .bound(&key(bytes))
                .map(|(id, _)| id),
            Some(expected_id),
            "{ctx}: the loser's creation takes the head's next id"
        );
    }
    // Branch cursors: a page, then a rejected write (the serial does not
    // advance), then the cursor still works; a rollback invalidates it.
    let query = Query::match_all();
    let model = &pending.view[&namespace];
    let all = model.matches(&query);
    let mut checked = Checked::default();
    let walk = PageWalk {
        reader: &branch,
        model,
        weights: &weights,
        namespace,
        label: "loser/match-all",
        query: &query,
        projection: &Projection::ID_ONLY,
        all: &all,
        ctx: &ctx,
    };
    let cursor = walk.check_page(PAGE, None, true, &mut checked);
    execute_checked(
        &mut branch,
        &mut pending,
        Path::Owner(BOB),
        delete_record(namespace, bytes),
        Budget::unlimited(),
        true,
        &ctx,
        transcript,
        coverage,
    );
    if let Some(cursor) = &cursor {
        assert_eq!(
            cursor.snapshot(),
            None,
            "{ctx}: a branch cursor names no snapshot"
        );
        let walk = PageWalk {
            reader: &branch,
            model: &pending.view[&namespace],
            weights: &weights,
            namespace,
            label: "loser/match-all",
            query: &query,
            projection: &Projection::ID_ONLY,
            all: &all,
            ctx: &ctx,
        };
        walk.check_page(PAGE, Some(cursor), true, &mut checked);
    }
    let checkpoint = branch.checkpoint();
    assert_eq!(
        branch.rollback_to(checkpoint),
        Ok(pending.receipts.len()..pending.receipts.len()),
        "{ctx}"
    );
    if let Some(cursor) = &cursor {
        let stale = branch.query(
            namespace,
            &query,
            &Projection::ID_ONLY,
            PAGE,
            Some(cursor),
            Budget::unlimited(),
        );
        assert_eq!(
            flat_read(&stale).0,
            cursor_mismatch(&weights),
            "{ctx}: a rollback invalidates branch cursors"
        );
    }
    execute_checked(
        &mut branch,
        &mut pending,
        Path::Host,
        create_namespace("test"),
        Budget::unlimited(),
        true,
        &ctx,
        transcript,
        coverage,
    );
    transcript.record_reads(&checked);
    transcript.record_pages(&checked);
    assert_eq!(branch.receipts(), pending.receipts.as_slice(), "{ctx}");
    branch.seal()
}

struct Retained {
    block: u64,
    id: SnapshotId,
    handle: Snapshot,
    view: View,
    checked: Checked,
}

/// Creates the two fixture namespaces in block 1 with the literal charge
/// DirectoryLookup 1 + {NamespaceCreate 50 + RootUpdate 2} = 53 under
/// FIXTURE.
fn create_namespaces(
    branch: &mut Branch,
    pending: &mut Pending,
    ctx: &Ctx,
    transcript: &mut Transcript<Event>,
) {
    for (ordinal, owner) in [(NS1, ALICE), (NS2, BOB)] {
        let cost = pending.weights.directory_lookup
            + pending.weights.namespace_create
            + pending.weights.root_update;
        let result = branch.execute(owner, create_namespace("test"), Budget::limit(cost));
        let (got, physical) = flat_write(&result);
        assert_eq!(
            got,
            Ok((
                Effect::NamespaceCreated(ns(ordinal)),
                receipt(
                    ReceiptKind::Write,
                    pending.weights.id,
                    cost,
                    Outcome::Applied
                )
            )),
            "{ctx}: namespace {ordinal}"
        );
        pending.record(Outcome::Applied, cost);
        pending.created.push((ns(ordinal), owner));
        pending.view.insert(ns(ordinal), NsModel::new(owner));
        transcript.events.push(Event::Write(got));
        transcript.physical.push(physical);
    }
}

/// Runs one seed on a fresh engine. `thorough` adds estimates, per-operation
/// branch checks and the final history pass; the transcript is the same
/// either way.
fn run_seed(seed: u32, thorough: bool) -> Run<Event, Coverage> {
    let mut generator = Generator(seed);
    let mut engine = Engine::with_limits(PriceSchedule::FIXTURE, LIMITS);
    assert_eq!(engine.limits(), LIMITS);
    let mut oracle = Oracle::new();
    let mut transcript = Transcript::new();
    let mut coverage = Coverage::default();
    let mut retained: Vec<Retained> = Vec::new();
    let mut next_snapshot = 1u64;
    let mut previous_cursor: Option<Issued> = None;

    assert_eq!(engine.head(), SnapshotId::GENESIS);
    {
        let ctx = Ctx::new(seed, 0, "genesis");
        let genesis = engine.head_snapshot();
        let (checked, _) = check_reads(&genesis, &oracle.head, &Weights::FIXTURE, &ctx);
        retained.push(Retained {
            block: 0,
            id: genesis.id(),
            handle: genesis,
            view: oracle.head.clone(),
            checked,
        });
    }

    for block in 1..=BLOCKS {
        if block.is_multiple_of(ADMIN_EVERY) {
            publish_admin(
                &mut engine,
                &mut oracle,
                block,
                &mut next_snapshot,
                &mut transcript,
                seed,
            );
        }
        let weights = schedule_at(&oracle.log.schedules, block);
        let size = block_size(block, &SIZES);
        let budget = block_budget(block, size);
        let base = engine.head();
        let mut branch = engine.begin(base, budget.map(Cost::new)).unwrap();
        let ctx = Ctx::new(seed, block, "execute");
        assert_eq!(branch.limits(), LIMITS, "{ctx}");
        assert_eq!(
            branch.pricing().id(),
            weights.id,
            "{ctx}: priced with the base's schedule"
        );
        let mut pending = Pending::open(&oracle, weights, budget);
        let focus = generator.pick(&KEYS);
        let mut checkpoints: Vec<Checkpoint> = Vec::new();
        let mut doomed = false;

        if block == 1 {
            create_namespaces(&mut branch, &mut pending, &ctx, &mut transcript);
        } else {
            for index in 0..size {
                if index % TRANSACTION_OPS == 0 {
                    checkpoints.push(branch.checkpoint());
                    pending.checkpoint();
                    doomed = generator.chance(4);
                }
                let (path, op, op_budget) = plan_op(&mut generator, block, index, focus, &pending);
                execute_checked(
                    &mut branch,
                    &mut pending,
                    path,
                    op,
                    op_budget,
                    thorough,
                    &ctx,
                    &mut transcript,
                    &mut coverage,
                );
                let closes = index % TRANSACTION_OPS == TRANSACTION_OPS - 1 || index + 1 == size;
                if doomed && closes {
                    let ctx = Ctx::new(seed, block, "rollback");
                    let last = checkpoints.len() - 1;
                    rollback_checked(
                        &mut branch,
                        &mut pending,
                        &checkpoints,
                        last,
                        &ctx,
                        &mut transcript,
                        &mut coverage,
                    );
                    doomed = false;
                }
            }
            // Occasionally unwind several transactions at once: checkpoints
            // taken after the target become invalid, the target stays usable.
            if checkpoints.len() >= 2 && generator.chance(6) {
                let ctx = Ctx::new(seed, block, "unwind");
                let target = generator.index(checkpoints.len() - 1);
                let stale = checkpoints[checkpoints.len() - 1];
                rollback_checked(
                    &mut branch,
                    &mut pending,
                    &checkpoints,
                    target,
                    &ctx,
                    &mut transcript,
                    &mut coverage,
                );
                assert_eq!(
                    branch.rollback_to(stale),
                    Err(EngineError::InvalidCheckpoint),
                    "{ctx}: a checkpoint taken after the rollback target is invalid"
                );
                checkpoints.truncate(target + 1);
            }
        }

        // The working state before expiry, read through the branch.
        if thorough {
            let ctx = Ctx::new(seed, block, "branch");
            check_reads(&branch, &pending.view, &weights, &ctx);
        }

        // Host expiry before the seal: a starved first run halts on its
        // first delete, the regular run finishes the job (or halts on the
        // block budget, leaving the rest to a later block).
        let ctx = Ctx::new(seed, block, "expiry");
        let mut expired: Vec<(NamespaceId, RecordId, RecordKey)> = Vec::new();
        for namespace in [ns(NS1), ns(NS2)] {
            let mut ids = Vec::new();
            if block.is_multiple_of(STARVE_EVERY) {
                ids.extend(expire_checked(
                    &mut branch,
                    &mut pending,
                    namespace,
                    block,
                    true,
                    &ctx,
                    &mut transcript,
                    &mut coverage,
                ));
            }
            ids.extend(expire_checked(
                &mut branch,
                &mut pending,
                namespace,
                block,
                false,
                &ctx,
                &mut transcript,
                &mut coverage,
            ));
            for id in ids {
                let key = retained
                    .last()
                    .and_then(|previous| previous.view.get(&namespace))
                    .and_then(|model| model.rows.get(&id))
                    .map(|rec| rec.key.clone());
                let key = key.unwrap_or_else(|| {
                    // Created in this block: find the key in the pending log.
                    pending
                        .mutations
                        .iter()
                        .find_map(|(n, m)| match m {
                            Mutation::Create { key, id: c, .. } if *n == namespace && *c == id => {
                                Some(key.clone())
                            }
                            _ => None,
                        })
                        .expect("an expired row was created at some point")
                });
                expired.push((namespace, id, key));
            }
        }

        // Seal: receipts and totals are the oracle's, whatever was rolled back.
        let ctx = Ctx::new(seed, block, "seal");
        let sealed = seal_checked(branch, base, &pending.receipts, pending.spent(), &ctx);

        // Staged: the head still shows only the published log.
        let ctx = Ctx::new(seed, block, "staged");
        let head = engine.head_snapshot();
        assert_eq!(head.id(), base, "{ctx}");
        let (checked, _) = check_reads(&head, &oracle.head, &weights, &ctx);
        transcript.record_reads(&checked);

        let loser = block.is_multiple_of(LOSER_EVERY).then(|| {
            build_loser(
                &engine,
                &oracle,
                &mut generator,
                weights,
                seed,
                block,
                &mut transcript,
                &mut coverage,
            )
        });

        let ctx = Ctx::new(seed, block, "publish");
        let (id, refusal) = publish_checked(
            &mut engine,
            &sealed,
            loser.as_ref(),
            &mut next_snapshot,
            &ctx,
        );
        transcript.events.push(Event::Published(id));
        let touched = oracle.commit(block, pending);
        transcript.events.extend(refusal.map(Event::StaleHead));

        // Applied: the new head is exactly the log.
        let ctx = Ctx::new(seed, block, "applied");
        let snapshot = engine.snapshot(id).unwrap();
        assert_eq!(snapshot.id(), id, "{ctx}");
        assert_eq!(snapshot.base(), Some(base), "{ctx}");
        assert_eq!(snapshot.pricing().id(), weights.id, "{ctx}");
        let (checked, issued) = check_reads(&snapshot, &oracle.head, &weights, &ctx);
        transcript.record_reads(&checked);
        transcript.record_pages(&checked);

        // Expired rows: absent now, present in the previous block's snapshot
        // when they were live there.
        let previous = retained.last().expect("genesis is retained");
        for (namespace, id, record_key) in &expired {
            let now = snapshot
                .get_record(
                    *namespace,
                    record_key,
                    &Projection::ID_ONLY,
                    Budget::unlimited(),
                )
                .unwrap_or_else(|rejected| panic!("{ctx}: {rejected}"));
            assert_eq!(
                now.value, None,
                "{ctx} ns={namespace}: expired {id} still bound"
            );
            assert_eq!(
                now.receipt,
                completed_read(&weights, weights.directory_lookup + weights.point_read),
                "{ctx}: an unbound key costs the lookup"
            );
            let was_live = previous
                .view
                .get(namespace)
                .is_some_and(|m| m.rows.contains_key(id));
            let then = previous
                .handle
                .get_record(
                    *namespace,
                    record_key,
                    &Projection::ID_ONLY,
                    Budget::unlimited(),
                )
                .map(|metered| metered.value.map(|row| row.id))
                .unwrap_or_else(|rejected| panic!("{ctx}: {rejected}"));
            assert_eq!(
                then,
                was_live.then_some(*id),
                "{ctx} ns={namespace}: {id} in the previous block"
            );
        }

        // Typed sharing is root identity of the key, row and term maps,
        // which are derived from the live rows alone: a namespace shares its
        // typed structure with the previous block exactly when the model's
        // rows (ids, keys and cells) are unchanged. That holds for every
        // namespace the block did not write, and also for a written one
        // whose writes cancelled out (a record created and deleted again, a
        // patch that set the values it already had).
        for namespace in [ns(NS1), ns(NS2)] {
            if block > 1 {
                let unchanged = previous.view[&namespace].rows == oracle.head[&namespace].rows;
                assert!(
                    touched.contains(&namespace) || unchanged,
                    "{ctx} ns={namespace}: the model changed an untouched namespace"
                );
                assert_eq!(
                    previous.handle.typed_shared_with(&snapshot, namespace),
                    unchanged,
                    "{ctx} ns={namespace}: typed sharing (touched={}, rows unchanged={unchanged})",
                    touched.contains(&namespace)
                );
            }
        }

        // A cursor from an earlier snapshot is a mismatch here.
        if let Some(previous) = &previous_cursor {
            let stale = snapshot.query(
                previous.cursor.namespace(),
                &previous.query,
                &previous.projection,
                PAGE,
                Some(&previous.cursor),
                Budget::unlimited(),
            );
            assert_eq!(
                flat_read(&stale).0,
                cursor_mismatch(&weights),
                "{ctx}: a cursor binds its snapshot"
            );
        }
        if issued.is_some() {
            previous_cursor = issued;
        }
        retained.push(Retained {
            block,
            id,
            handle: snapshot,
            view: oracle.head.clone(),
            checked,
        });
    }

    if thorough {
        verify_history(&engine, &oracle, &retained, seed, &mut coverage);
    }
    Run {
        transcript,
        coverage,
    }
}

/// After all blocks: every published block, from `engine.snapshot` and
/// from the handle retained at the boundary, must repeat the boundary's answers, receipts and pages against the view
/// replayed from the mutation log.
fn verify_history(
    engine: &Engine,
    oracle: &Oracle,
    retained: &[Retained],
    seed: u32,
    coverage: &mut Coverage,
) {
    for entry in retained {
        let block = entry.block;
        let view = oracle.log.view_at(block);
        assert_eq!(
            view, entry.view,
            "seed={seed:#010x} block={block}: replayed log vs boundary view"
        );
        let weights = schedule_at(&oracle.log.schedules, block);
        let ctx = Ctx::new(seed, block, "history");
        let snapshot = engine.snapshot(entry.id).unwrap();
        assert_eq!(snapshot.id(), entry.id, "{ctx}");
        assert_eq!(
            snapshot.pricing().id(),
            weights.id,
            "{ctx}: reads use the snapshot's schedule"
        );
        for (phase, handle) in [
            ("history/engine.snapshot", &snapshot),
            ("history/retained", &entry.handle),
        ] {
            let ctx = Ctx::new(seed, block, phase);
            let (checked, _) = check_reads(handle, &view, &weights, &ctx);
            assert_eq!(
                checked, entry.checked,
                "{ctx}: receipts and pages differ from the boundary"
            );
        }
        for (namespace, model) in &view {
            let head = &oracle.head[namespace];
            coverage.deleted_later += model
                .rows
                .keys()
                .filter(|id| !head.rows.contains_key(id))
                .count();
        }
    }
    assert_eq!(
        engine
            .snapshot(SnapshotId::GENESIS)
            .unwrap()
            .namespace_count(),
        0
    );
}

fn full_run(seed: u32) {
    let run = run_seed(seed, true);
    let coverage = &run.coverage;
    assert_verdicts_seen(
        &coverage.verdicts,
        &[
            "Applied",
            "AlreadyExists",
            "NotFound",
            "Unauthorized",
            "UnknownNamespace",
            "RecordFull",
            "ExceedsBlockBudget",
            "OutOfBudget",
            "InvalidArgument(TooManyCells)",
            "InvalidArgument(DuplicateCell)",
            "InvalidArgument(SetAndRemoveOverlap)",
            "InvalidArgument(ValueTooLarge)",
            "InvalidArgument(BytesNotIndexable)",
            "InvalidArgument(NotHostOperation)",
        ],
        seed,
        coverage,
    );
    for (what, value) in [
        ("type changes", coverage.type_changes),
        ("kind changes", coverage.kind_changes),
        ("rollbacks", coverage.rollbacks),
        ("released ids", coverage.released_ids),
        ("recreated keys", coverage.recreated_keys),
        ("expired rows", coverage.expired),
        ("same-block expiries", coverage.same_block_expiries),
        ("late expiries", coverage.late_expiries),
        ("multi-page expiries", coverage.multi_page_expiries),
        ("expiry halts", coverage.halts),
        ("field survivors", coverage.field_survivors),
        ("new terms", coverage.new_terms),
        ("reused terms", coverage.reused_terms),
        ("rows deleted later", coverage.deleted_later),
        ("writes under the 32-byte key", coverage.longest_key_writes),
    ] {
        assert!(value > 0, "seed={seed:#010x}: no {what}: {coverage:?}");
    }
}

fn replay(seed: u32) {
    let a = run_seed(seed, false).transcript;
    let b = run_seed(seed, false).transcript;
    assert_replay(&a, &b, seed);
    assert!(
        a.events
            .iter()
            .any(|event| matches!(event, Event::StaleHead(_))),
        "seed={seed:#010x}: the transcript records the losers"
    );
    assert!(
        a.events
            .iter()
            .any(|event| matches!(event, Event::Expiry(Err(_)))),
        "seed={seed:#010x}: the transcript records a halted expiry"
    );
}

/// The worked vectors: three records a, b, c with attributes kind
/// (Str), price (I64), $expiresAt (U64(5)) and a field note (Bytes "xyz");
/// kinds book, book, pen; prices 10, 20, 5.
fn worked_vectors() -> NsModel {
    let mut model = NsModel::new(ALICE);
    for (ordinal, bytes, kind, price) in [
        (1, b"a", "book", 10),
        (2, b"b", "book", 20),
        (3, b"c", "pen", 5),
    ] {
        model.apply(&Mutation::Create {
            key: key(bytes),
            id: rid(ordinal),
            cells: cells_of(&[
                (name(KIND), attr(Value::str(kind))),
                (name(PRICE), attr(Value::I64(price))),
                (name(EXPIRES), attr(Value::U64(5))),
                (name(NOTE), field(Value::bytes(b"xyz"))),
            ]),
        });
    }
    model
}

#[test]
fn charge_table_reproduces_the_worked_vectors() {
    let w = Weights::FIXTURE;
    let full = worked_vectors();
    let mut model = NsModel::new(ALICE);
    let mut created = Vec::new();
    for (id, rec) in &full.rows {
        created.push(total(
            &w.create_record_charges(&model, &rec.key, &rec.cells),
        ));
        model.apply(&Mutation::Create {
            key: rec.key.clone(),
            id: *id,
            cells: rec.cells.clone(),
        });
    }
    // The model-3 worked vectors: 302 + RowByte 47 + RootUpdate 2 * (3 +
    // 2 * 6) = 379; 152 + 47 + 30 = 229; 181 + 46 + 30 = 257.
    assert_eq!(created, vec![379, 229, 257]);
    assert_eq!(model, full);

    let kind_book = [Literal::of(Term::eq(name(KIND), Value::str("book")))];
    let page1 = predict_page(&w, &model, &kind_book, &Projection::ALL, 1, None);
    assert_eq!(
        (total(&page1.charges), page1.rows, page1.next),
        (62, vec![rid(1)], Some(rid(1)))
    );
    let page2 = predict_page(&w, &model, &kind_book, &Projection::ALL, 1, Some(rid(1)));
    assert_eq!(
        (total(&page2.charges), page2.rows, page2.next),
        (61, vec![rid(2)], None)
    );

    let above_five = [Literal::of(Term::range(
        name(PRICE),
        Bound::Excluded(Value::I64(5)),
        Bound::Unbounded,
    ))];
    let plan = predict_page(&w, &model, &above_five, &Projection::ID_ONLY, 10, None);
    assert_eq!(
        (total(&plan.charges), plan.rows),
        (24, vec![rid(1), rid(2)])
    );

    let not_bo = [Literal::not(Term::starts_with(name(KIND), "bo"))];
    let plan = predict_page(&w, &model, &not_bo, &Projection::KEY_ONLY, 10, None);
    assert_eq!((total(&plan.charges), plan.rows), (19, vec![rid(3)]));

    assert_eq!(total(&predict_count(&w, &model, &[]).charges), 9);
    let exists = [Literal::of(Term::exists(name(PRICE), TypeTag::I64))];
    assert_eq!(total(&predict_count(&w, &model, &exists).charges), 13);

    let set = vec![(name(PRICE), attr(Value::U64(10)))];
    let remove = vec![name(NOTE), name("missing")];
    let a = &model.rows[&rid(1)];
    // 123 + RowByte 40 (key 1 + $expiresAt 18 + kind 8 + price 13) +
    // RootUpdate 2 * (2 + 4 * 2) = 183.
    assert_eq!(
        total(&w.patch_record_charges(&model, rid(1), &a.key, &a.cells, &set, &remove)),
        183
    );
    model.apply(&Mutation::Patch {
        id: rid(1),
        set,
        remove,
    });
    let deletes: Vec<u64> = model
        .rows
        .values()
        .map(|rec| total(&w.delete_record_charges(&rec.cells)))
        .collect();
    // 72 + RootUpdate 2 * (3 + 6 * 2) = 102; 76 + 30 = 106 twice.
    assert_eq!(deletes, vec![102, 106, 106]);
    let bytes = row_bytes(&project(rid(1), &full.rows[&rid(1)], &Projection::ALL));
    assert_eq!(total(&w.get_record_charges(Some(bytes))), 54);

    // Verdict rules: the exact budget applies, one unit less stops before
    // the last group, the semantic checks sit after their charge.
    let a = &full.rows[&rid(1)];
    let charges = w.create_record_charges(&NsModel::new(ALICE), &a.key, &a.cells);
    assert_eq!(verdict(&charges, None, 379), (Outcome::Applied, 379));
    // 379 - the last insert {6 + 30 + 4} = 339.
    assert_eq!(
        verdict(&charges, None, 378),
        (Outcome::Rejected(Rejection::OutOfBudget), 339)
    );
    assert_eq!(
        verdict(&[1, 3], Some((1, Rejection::AlreadyExists)), 378),
        (Outcome::Rejected(Rejection::AlreadyExists), 4)
    );
    assert_eq!(
        verdict(&[1], Some((0, Rejection::Unauthorized)), 0),
        (Outcome::Rejected(Rejection::OutOfBudget), 0)
    );
}

#[test]
fn reference_evaluator_follows_the_query_semantics() {
    let model = worked_vectors();
    let n = name;
    let matches = |term: Term| model.matches(&Query::term(term));
    assert_eq!(
        matches(Term::eq(n(KIND), Value::str("book"))),
        vec![rid(1), rid(2)]
    );
    // A field never matches, whatever its type.
    assert_eq!(
        matches(Term::exists(n(NOTE), TypeTag::Bytes)),
        Vec::<RecordId>::new()
    );
    // NotEq needs an attribute of the value's type.
    assert_eq!(
        matches(Term::not_eq(n(KIND), Value::str("book"))),
        vec![rid(3)]
    );
    assert_eq!(
        matches(Term::not_eq(n(PRICE), Value::U64(10))),
        Vec::<RecordId>::new()
    );
    assert_eq!(
        matches(Term::not_eq(n(GHOST), Value::Bool(true))),
        Vec::<RecordId>::new()
    );
    // Ranges honour Included/Excluded/Unbounded within the bounds' type.
    let range = |lower, upper| matches(Term::range(n(PRICE), lower, upper));
    assert_eq!(
        range(
            Bound::Included(Value::I64(5)),
            Bound::Excluded(Value::I64(20))
        ),
        vec![rid(1), rid(3)]
    );
    assert_eq!(
        range(Bound::Excluded(Value::I64(5)), Bound::Unbounded),
        vec![rid(1), rid(2)]
    );
    assert_eq!(
        range(Bound::Unbounded, Bound::Included(Value::U64(20))),
        Vec::<RecordId>::new()
    );
    assert_eq!(
        range(
            Bound::Included(Value::I64(20)),
            Bound::Included(Value::I64(5))
        ),
        Vec::<RecordId>::new()
    );
    // Prefixes are byte-wise; the empty prefix matches every Str attribute.
    assert_eq!(
        matches(Term::starts_with(n(KIND), "")),
        vec![rid(1), rid(2), rid(3)]
    );
    assert_eq!(matches(Term::starts_with(n(KIND), "pe")), vec![rid(3)]);
    assert_eq!(
        matches(Term::starts_with(n(PRICE), "")),
        Vec::<RecordId>::new()
    );
    // Negation is the complement over all live records; groups AND, queries OR.
    let negated = Query::all(vec![Literal::not(Term::eq(n(KIND), Value::str("book")))]);
    assert_eq!(model.matches(&negated), vec![rid(3)]);
    let both = Query::all(vec![
        Literal::of(Term::eq(n(KIND), Value::str("book"))),
        Literal::not(Term::eq(n(PRICE), Value::I64(10))),
    ]);
    assert_eq!(model.matches(&both), vec![rid(2)]);
    let either = Query {
        groups: vec![
            vec![Literal::of(Term::eq(n(KIND), Value::str("pen")))],
            vec![Literal::of(Term::eq(n(PRICE), Value::I64(10)))],
        ],
    };
    assert_eq!(model.matches(&either), vec![rid(1), rid(3)]);
    assert_eq!(
        model.matches(&Query::match_all()),
        vec![rid(1), rid(2), rid(3)]
    );
    // kind: book, pen + existence; price: 10, 20, 5 + existence; $expiresAt: 5 +
    // existence; note is a field and holds no term.
    assert_eq!(model.term_count(), (2 + 1) + (3 + 1) + (1 + 1));
}

/// A host that lets a user write `$`-cells loses expiry: a patch that turns
/// `$expiresAt` into a field makes the record survive `expire_records`,
/// exactly as the reference evaluator predicts. A host is expected to refuse
/// user writes to `$`-cells before they reach the engine.
#[test]
fn a_field_expires_at_survives_expiry_without_host_filtering() {
    let w = Weights::FIXTURE;
    let mut engine = Engine::new(PriceSchedule::FIXTURE);
    let namespace = ns(1);
    let expires = name(EXPIRES);
    let cells = |bytes: &[u8]| {
        vec![
            (expires.clone(), attr(Value::U64(3))),
            (name(KIND), attr(Value::str("book"))),
            (name(NOTE), field(Value::bytes(bytes))),
        ]
    };
    let mut model = NsModel::new(ALICE);
    let mut branch = engine.begin(engine.head(), None).unwrap();
    branch
        .execute(ALICE, create_namespace("test"), Budget::unlimited())
        .unwrap();
    // Block 1: two records, both stamped to expire at block 3. The host
    // path writes them, so the `$` cell is the host's own.
    for (ordinal, bytes) in [(1, b"a".as_slice()), (2, b"b")] {
        let op = create_record(namespace, bytes, cells(b"xyz"));
        let want = total(&w.create_record_charges(&model, &key(bytes), &cells_of(&cells(b"xyz"))));
        let executed = branch.execute_host(op, Budget::unlimited()).unwrap();
        assert_eq!(executed.effect, Effect::RecordCreated { id: rid(ordinal) });
        assert_eq!(executed.receipt, applied(want));
        model.apply(&Mutation::Create {
            key: key(bytes),
            id: rid(ordinal),
            cells: cells_of(&cells(b"xyz")),
        });
    }
    // Namespace: 1 + 50 + 2 = 53. Records: 1 + 3 + {20 + 12 + (10+8 + 4+4 +
    // 4+3 = 33) + RowByte (1 + 33) + RootUpdate 6 = 105} + 2 attributes × 2
    // terms: a creates 4 terms (1+3+105+4*40 = 269), b reuses all
    // (1+3+105+4*10 = 149).
    assert_eq!(
        branch
            .receipts()
            .iter()
            .map(|r| r.spent.units())
            .collect::<Vec<_>>(),
        vec![53, 269, 149]
    );
    engine.publish(&branch.seal()).unwrap();

    // Block 2: a user patch (owner path) turns b's `$expiresAt` into a
    // field. The engine accepts it: 1 + 3 + {10 + 4 + 18 + RowByte 34 (the
    // row keeps its size) + RootUpdate 4} + {IndexRemove 6 + RootUpdate 4}
    // for the value term and the existence term = 4 + 70 + 20 = 94.
    let mut branch = engine.begin(engine.head(), None).unwrap();
    let op = patch_record(
        namespace,
        b"b",
        vec![(expires.clone(), field(Value::U64(3)))],
        vec![],
    );
    let b = &model.rows[&rid(2)].cells;
    let set = vec![(expires.clone(), field(Value::U64(3)))];
    assert_eq!(
        total(&w.patch_record_charges(&model, rid(2), &key(b"b"), b, &set, &[])),
        94
    );
    let executed = branch.execute(ALICE, op, Budget::unlimited()).unwrap();
    assert_eq!(executed.effect, Effect::RecordPatched { id: rid(2) });
    assert_eq!(executed.receipt, applied(94));
    model.apply(&Mutation::Patch {
        id: rid(2),
        set,
        remove: vec![],
    });
    assert_eq!(branch.index_consistent(namespace), Some(true));
    engine.publish(&branch.seal()).unwrap();

    // Block 3: expiry deletes a only. The reference evaluator agrees that
    // b's field cell never satisfies the range term.
    let due = Query::term(Term::range(
        expires.clone(),
        Bound::Unbounded,
        Bound::Included(Value::U64(3)),
    ));
    assert_eq!(model.matches(&due), vec![rid(1)]);
    let mut branch = engine.begin(engine.head(), None).unwrap();
    let delete_a = total(&w.delete_record_charges(&model.rows[&rid(1)].cells));
    // 1 + 3 + {20 + 12 + RootUpdate 6} + 4 × {IndexRemove 6 + RootUpdate 4}
    // = 82.
    assert_eq!(delete_a, 82);
    let expired = expire_records(
        &mut branch,
        namespace,
        &expires,
        BlockNumber(3),
        Budget::unlimited(),
        Budget::unlimited(),
    )
    .unwrap();
    assert_eq!(expired, vec![(rid(1), applied(82))]);
    model.apply(&Mutation::Delete { id: rid(1) });
    let survivor = branch
        .get_record(namespace, &key(b"b"), &Projection::ALL, Budget::unlimited())
        .unwrap();
    let row = project(rid(2), &model.rows[&rid(2)], &Projection::ALL);
    let bytes = row_bytes(&row);
    assert_eq!(survivor.value, Some(row));
    assert_eq!(
        survivor.receipt,
        read_applied(total(&w.get_record_charges(Some(bytes))))
    );
    // Block 4 finds nothing either: the field is invisible to every term.
    let again = expire_records(
        &mut branch,
        namespace,
        &expires,
        BlockNumber(4),
        Budget::unlimited(),
        Budget::unlimited(),
    )
    .unwrap();
    assert!(again.is_empty());
    assert_eq!(
        branch
            .count(namespace, &due, Budget::unlimited())
            .unwrap()
            .value,
        0
    );
    assert_eq!(branch.typed_record_count(namespace), Some(1));
    assert_eq!(branch.index_consistent(namespace), Some(true));
}

seed_tests! {
    query_history_matches_oracle_seed_00000007, replay_is_deterministic_seed_00000007 => SEEDS[0];
    query_history_matches_oracle_seed_9e3779b9, replay_is_deterministic_seed_9e3779b9 => SEEDS[1];
    query_history_matches_oracle_seed_deadbeef, replay_is_deterministic_seed_deadbeef => SEEDS[2];
}
