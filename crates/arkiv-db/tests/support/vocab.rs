//! The words the tests are written in: owners, ids, write operations,
//! receipts and the few branch steps every file needs.

use arkiv_db::{
    Branch, Budget, Cell, CellName, Cost, Effect, Engine, Entry, Executed, Ledger, LedgerEntry,
    Limits, NamespaceDescriptor, NamespaceId, NamespaceName, OperationClass, Outcome, OwnerId,
    PhysicalTrace, PriceSchedule, ReadResult, Receipt, ReceiptKind, RecordId, RecordKey, Rejected,
    Rejection, Revision, SnapshotId, Value, WriteOp, WriteResult,
};

pub const ALICE: OwnerId = OwnerId::new([0xa1; 20]);
pub const BOB: OwnerId = OwnerId::new([0xb0; 20]);
pub const CAROL: OwnerId = OwnerId::new([0xc0; 20]);
pub const DAVE: OwnerId = OwnerId::new([0xd0; 20]);
/// The owner in tests where ownership plays no part.
pub const OWNER: OwnerId = OwnerId::new([7; 20]);

/// Counting-rule version recorded in every receipt.
pub const MODEL_VERSION: u32 = 3;
/// The id `PriceSchedule::FIXTURE` carries.
pub const FIXTURE_ID: u32 = 1;

pub const OUT_OF_BUDGET: Outcome = Outcome::Rejected(Rejection::OutOfBudget);
pub const COST_OVERFLOW: Outcome = Outcome::Rejected(Rejection::CostOverflow);

pub type Cells = Vec<(CellName, Cell)>;

pub fn ns(ordinal: u64) -> NamespaceId {
    NamespaceId::from_ordinal(ordinal).expect("namespace ordinals start at 1")
}

pub fn rid(ordinal: u64) -> RecordId {
    RecordId::from_ordinal(ordinal).expect("record ordinals start at 1")
}

pub fn name(text: &str) -> CellName {
    CellName::new(text).unwrap_or_else(|fault| panic!("{text:?} is not a cell name: {fault:?}"))
}

pub fn names(texts: &[&str]) -> Vec<CellName> {
    texts.iter().copied().map(name).collect()
}

pub fn key(bytes: impl AsRef<[u8]>) -> RecordKey {
    RecordKey::new(bytes.as_ref()).expect("record keys are 1..=32 bytes")
}

pub fn attr(value: Value) -> Cell {
    Cell::attribute(value)
}

pub fn field(value: Value) -> Cell {
    Cell::field(value)
}

pub fn namespace_name(text: &str) -> NamespaceName {
    NamespaceName::new(text).expect("a valid namespace name")
}

pub fn create_namespace(name: &str) -> WriteOp {
    WriteOp::CreateNamespace {
        name: namespace_name(name),
    }
}

pub fn transfer(namespace: NamespaceId, expected_revision: u64, new_owner: OwnerId) -> WriteOp {
    WriteOp::TransferNamespace {
        namespace,
        expected_revision: Revision(expected_revision),
        new_owner,
    }
}

pub fn put(namespace: NamespaceId, key: &[u8], value: &[u8]) -> WriteOp {
    WriteOp::Put {
        namespace,
        key: key.to_vec(),
        value: value.to_vec(),
    }
}

pub fn delete(namespace: NamespaceId, key: &[u8]) -> WriteOp {
    WriteOp::Delete {
        namespace,
        key: key.to_vec(),
    }
}

pub fn create_record(namespace: NamespaceId, key: impl AsRef<[u8]>, cells: Cells) -> WriteOp {
    WriteOp::CreateRecord {
        namespace,
        key: self::key(key),
        cells,
    }
}

pub fn patch_record(
    namespace: NamespaceId,
    key: impl AsRef<[u8]>,
    set: Cells,
    remove: Vec<CellName>,
) -> WriteOp {
    WriteOp::PatchRecord {
        namespace,
        key: self::key(key),
        set,
        remove,
    }
}

pub fn delete_record(namespace: NamespaceId, key: impl AsRef<[u8]>) -> WriteOp {
    WriteOp::DeleteRecord {
        namespace,
        key: self::key(key),
    }
}

pub fn entry(key: &[u8], value: &[u8]) -> Entry {
    Entry {
        key: key.to_vec(),
        value: value.to_vec(),
    }
}

/// The descriptor of a namespace created with the name `"test"`.
pub fn descriptor(id: NamespaceId, owner: OwnerId, revision: u64) -> NamespaceDescriptor {
    NamespaceDescriptor {
        name: namespace_name("test"),
        id,
        owner,
        revision: Revision(revision),
    }
}

pub fn receipt(kind: ReceiptKind, schedule_id: u32, spent: u64, outcome: Outcome) -> Receipt {
    Receipt {
        kind,
        model_version: MODEL_VERSION,
        schedule_id,
        spent: Cost::new(spent),
        outcome,
        ledger: None,
    }
}

/// A write receipt under the fixture schedule.
pub fn write_receipt(spent: u64, outcome: Outcome) -> Receipt {
    receipt(ReceiptKind::Write, FIXTURE_ID, spent, outcome)
}

/// A read receipt under the fixture schedule.
pub fn read_receipt(spent: u64, outcome: Outcome) -> Receipt {
    receipt(ReceiptKind::Read, FIXTURE_ID, spent, outcome)
}

pub fn applied(spent: u64) -> Receipt {
    write_receipt(spent, Outcome::Applied)
}

pub fn rejected(spent: u64, reason: Rejection) -> Receipt {
    write_receipt(spent, Outcome::Rejected(reason))
}

pub fn read_applied(spent: u64) -> Receipt {
    read_receipt(spent, Outcome::Applied)
}

pub fn read_rejected(spent: u64, reason: Rejection) -> Receipt {
    read_receipt(spent, Outcome::Rejected(reason))
}

/// A read refused at admission: nothing charged, nothing visited.
pub fn read_admission(reason: Rejection) -> Rejected {
    Rejected {
        reason,
        receipt: read_rejected(0, reason),
        physical: PhysicalTrace::default(),
    }
}

/// Ledger lines as `(class, count, unit_price, subtotal)`.
pub fn ledger(lines: &[(OperationClass, u64, u64, u64)]) -> Ledger {
    Ledger {
        entries: lines
            .iter()
            .map(|&(class, count, unit_price, subtotal)| LedgerEntry {
                class,
                count,
                unit_price,
                subtotal: Cost::new(subtotal),
            })
            .collect(),
    }
}

/// Ledger lines as `(class, count, unit_price)`; each subtotal is
/// `count * unit_price`.
pub fn priced(lines: &[(OperationClass, u64, u64)]) -> Ledger {
    Ledger {
        entries: lines
            .iter()
            .map(|&(class, count, unit_price)| LedgerEntry {
                class,
                count,
                unit_price,
                subtotal: Cost::new(count * unit_price),
            })
            .collect(),
    }
}

/// How many units of `class` a ledger charged; zero if it has no such line.
pub fn charged(ledger: &Ledger, class: OperationClass) -> u64 {
    ledger
        .entries
        .iter()
        .find(|entry| entry.class == class)
        .map_or(0, |entry| entry.count)
}

pub fn traced(receipt: Receipt, ledger: Ledger) -> Receipt {
    Receipt {
        ledger: Some(ledger),
        ..receipt
    }
}

pub fn without_ledger(receipt: &Receipt) -> Receipt {
    Receipt {
        ledger: None,
        ..receipt.clone()
    }
}

/// The canonical part of a write result: effect or reason plus the receipt.
/// The physical trace is deliberately dropped.
pub type Canonical = Result<(Effect, Receipt), (Rejection, Receipt)>;

pub fn canonical(result: &WriteResult) -> Canonical {
    match result {
        Ok(executed) => Ok((executed.effect, executed.receipt.clone())),
        Err(rejected) => Err((rejected.reason, rejected.receipt.clone())),
    }
}

/// The canonical part of a read result: value or reason plus the receipt.
pub type CanonicalRead<T> = Result<(T, Receipt), (Rejection, Receipt)>;

pub fn canonical_read<T>(result: ReadResult<T>) -> CanonicalRead<T> {
    match result {
        Ok(metered) => Ok((metered.value, metered.receipt)),
        Err(rejected) => Err((rejected.reason, rejected.receipt)),
    }
}

/// A candidate on the current head.
pub fn begin(engine: &Engine, block_budget: Option<u64>) -> Branch {
    engine
        .begin(engine.head(), block_budget.map(Cost::new))
        .expect("begin on the current head")
}

pub fn publish(engine: &mut Engine, branch: Branch) -> SnapshotId {
    engine
        .publish(&branch.seal())
        .expect("the candidate is based on the head")
}

/// Executes `op` with an unlimited budget; it must apply.
pub fn apply(branch: &mut Branch, actor: OwnerId, op: WriteOp) -> Executed {
    branch
        .execute(actor, op.clone(), Budget::unlimited())
        .unwrap_or_else(|rejected| panic!("{actor:?} executing {op:?} was rejected: {rejected}"))
}

/// Executes `op` with an unlimited budget; it must be rejected.
pub fn reject(branch: &mut Branch, actor: OwnerId, op: WriteOp) -> Rejected {
    match branch.execute(actor, op.clone(), Budget::unlimited()) {
        Err(rejected) => rejected,
        Ok(executed) => panic!("{op:?} by {actor:?} applied unexpectedly: {executed:?}"),
    }
}

/// Creates a namespace for `owner` and returns its id.
pub fn add_namespace(branch: &mut Branch, owner: OwnerId, name: &str) -> NamespaceId {
    match apply(branch, owner, create_namespace(name)).effect {
        Effect::NamespaceCreated(id) => id,
        other => panic!("creation reports the new id, got {other:?}"),
    }
}

/// Snapshot 1 of a FIXTURE-priced engine with `limits`: one empty namespace
/// (ordinal 1) owned by ALICE.
pub fn engine_with_namespace(limits: Limits) -> (Engine, NamespaceId) {
    let mut engine = Engine::with_limits(PriceSchedule::FIXTURE, limits);
    let mut branch = begin(&engine, None);
    let namespace = add_namespace(&mut branch, ALICE, "test");
    assert_eq!(namespace, ns(1));
    assert_eq!(publish(&mut engine, branch).sequence(), 1);
    (engine, namespace)
}
