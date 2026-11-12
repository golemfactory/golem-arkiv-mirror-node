//! The frame the generated-history oracles share: unit prices as plain
//! numbers, the charge walk, flattened results and the engine-facing steps
//! every block goes through. What a history holds and how its results are
//! predicted stays in each test.

use std::{collections::BTreeMap, fmt};

use arkiv_db::{
    Branch, Budget, Cost, Effect, Engine, EngineError, OperationClass, Outcome, PhysicalTrace,
    PriceSchedule, ReadResult, Receipt, ReceiptKind, Rejection, SealedCandidate, Snapshot,
    SnapshotId, WriteResult,
};

use super::{
    pricing::price,
    random::Generator,
    vocab::{FIXTURE_ID, receipt},
};

pub fn count(items: usize) -> u64 {
    u64::try_from(items).expect("counts fit u64")
}

pub fn total(charges: &[u64]) -> u64 {
    charges.iter().sum()
}

macro_rules! weights {
    ($($field:ident: $class:ident,)*) => {
        /// A price schedule as the oracles see it: an id and one unit price
        /// per operation class, in plain u64 arithmetic. The engine's
        /// pricing API is never consulted.
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub struct Weights {
            pub id: u32,
            $(pub $field: u64,)*
        }

        impl Weights {
            /// The weights of `PriceSchedule::FIXTURE`.
            pub const FIXTURE: Self = Self {
                id: FIXTURE_ID,
                $($field: price(OperationClass::$class),)*
            };

            pub const fn uniform(id: u32, weight: u64) -> Self {
                Self {
                    id,
                    $($field: weight,)*
                }
            }
        }
    };
}

// ProofNode and ProofByte are left out: no history requests a proof.
weights! {
    directory_lookup: DirectoryLookup,
    namespace_create: NamespaceCreate,
    namespace_transfer: NamespaceTransfer,
    put_record: PutRecord,
    delete_record: DeleteRecord,
    write_key_byte: WriteKeyByte,
    write_value_byte: WriteValueByte,
    point_read: PointRead,
    scan_seek: ScanSeek,
    scan_entry: ScanEntry,
    read_byte: ReadByte,
    record_create: RecordCreate,
    record_patch: RecordPatch,
    record_delete: RecordDelete,
    cell_write: CellWrite,
    cell_byte: CellByte,
    cell_remove: CellRemove,
    index_insert: IndexInsert,
    index_term_create: IndexTermCreate,
    index_remove: IndexRemove,
    query_group: QueryGroup,
    term_seek: TermSeek,
    term_visit: TermVisit,
    posting_entry: PostingEntry,
    row_visit: RowVisit,
    record_probe: RecordProbe,
    result_row: ResultRow,
    root_update: RootUpdate,
    row_byte: RowByte,
}

/// The schedule in force at `block`, given `(first block, weights)` entries
/// in block order.
pub fn schedule_at(schedules: &[(u64, Weights)], block: u64) -> Weights {
    schedules
        .iter()
        .rev()
        .find(|(at, _)| *at <= block)
        .map(|(_, weights)| *weights)
        .expect("the genesis schedule is always recorded")
}

/// Walks the charge points. A charge that would push the running total past
/// `limit` stops the operation with OutOfBudget and the total unchanged; a
/// semantic rejection fires right after the charge with its index.
pub fn verdict(
    charges: &[u64],
    semantic: Option<(usize, Rejection)>,
    limit: u64,
) -> (Outcome, u64) {
    let mut spent = 0u64;
    for (index, &charge) in charges.iter().enumerate() {
        if spent + charge > limit {
            return (Outcome::Rejected(Rejection::OutOfBudget), spent);
        }
        spent += charge;
        if let Some((at, reason)) = semantic
            && at == index
        {
            return (Outcome::Rejected(reason), spent);
        }
    }
    (Outcome::Applied, spent)
}

/// A metered result without its physical trace.
pub type Flat<T> = Result<(T, Receipt), (Rejection, Receipt)>;

pub fn flat_write(result: &WriteResult) -> (Flat<Effect>, PhysicalTrace) {
    match result {
        Ok(executed) => (
            Ok((executed.effect, executed.receipt.clone())),
            executed.physical,
        ),
        Err(rejected) => (
            Err((rejected.reason, rejected.receipt.clone())),
            rejected.physical,
        ),
    }
}

pub fn flat_read<T: Clone>(result: &ReadResult<T>) -> (Flat<T>, PhysicalTrace) {
    match result {
        Ok(metered) => (
            Ok((metered.value.clone(), metered.receipt.clone())),
            metered.physical,
        ),
        Err(rejected) => (
            Err((rejected.reason, rejected.receipt.clone())),
            rejected.physical,
        ),
    }
}

pub fn receipt_of<T>(flat: &Flat<T>) -> Receipt {
    match flat {
        Ok((_, receipt)) | Err((_, receipt)) => receipt.clone(),
    }
}

/// An operation rejected with `reason` after spending `spent`.
pub fn rejection<T>(kind: ReceiptKind, schedule_id: u32, spent: u64, reason: Rejection) -> Flat<T> {
    Err((
        reason,
        receipt(kind, schedule_id, spent, Outcome::Rejected(reason)),
    ))
}

/// Expected read result for `value` under `charges`, with a semantic check
/// after the charge at `semantic.0`, priced with `schedule_id`.
pub fn expected_read<T>(
    value: T,
    charges: &[u64],
    semantic: Option<(usize, Rejection)>,
    limit: u64,
    schedule_id: u32,
) -> Flat<T> {
    let (outcome, spent) = verdict(charges, semantic, limit);
    let receipt = receipt(ReceiptKind::Read, schedule_id, spent, outcome);
    match outcome {
        Outcome::Applied => Ok((value, receipt)),
        Outcome::Rejected(reason) => Err((reason, receipt)),
    }
}

/// The name a write's verdict is counted under: the rejection without the
/// numbers it carries, an argument fault with its kind.
pub fn verdict_name(flat: &Flat<Effect>) -> String {
    match flat {
        Ok(_) => "Applied".to_string(),
        Err((Rejection::InvalidArgument(fault), _)) => format!("InvalidArgument({fault:?})"),
        Err((Rejection::ExceedsBlockBudget { .. }, _)) => "ExceedsBlockBudget".to_string(),
        Err((Rejection::StaleRevision { .. }, _)) => "StaleRevision".to_string(),
        Err((reason, _)) => format!("{reason:?}"),
    }
}

pub fn assert_verdicts_seen(
    verdicts: &BTreeMap<String, usize>,
    names: &[&str],
    seed: u32,
    coverage: &impl fmt::Debug,
) {
    for name in names {
        assert!(
            verdicts.get(*name).copied().unwrap_or(0) > 0,
            "seed={seed:#010x}: the workload never produced {name}: {coverage:?}"
        );
    }
}

/// Where in a run an assertion failed.
pub struct Ctx {
    seed: u32,
    block: u64,
    phase: &'static str,
}

impl Ctx {
    pub fn new(seed: u32, block: u64, phase: &'static str) -> Self {
        Self { seed, block, phase }
    }
}

impl fmt::Display for Ctx {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "seed={:#010x} block={} phase={}",
            self.seed, self.block, self.phase
        )
    }
}

/// What a run leaves behind for the replay comparison: its canonical events
/// in order and, apart from them, the physical trace of every write.
#[derive(Debug)]
pub struct Transcript<E> {
    pub events: Vec<E>,
    pub physical: Vec<PhysicalTrace>,
}

impl<E> Transcript<E> {
    pub fn new() -> Self {
        Self {
            events: Vec::new(),
            physical: Vec::new(),
        }
    }
}

pub struct Run<E, C> {
    pub transcript: Transcript<E>,
    pub coverage: C,
}

pub fn assert_replay<E: PartialEq + fmt::Debug>(
    first: &Transcript<E>,
    second: &Transcript<E>,
    seed: u32,
) {
    for (index, (x, y)) in first.events.iter().zip(&second.events).enumerate() {
        assert_eq!(
            x, y,
            "seed={seed:#010x}: transcript event {index} differs on replay"
        );
    }
    assert_eq!(
        first.events.len(),
        second.events.len(),
        "seed={seed:#010x}: transcript length"
    );
    assert_eq!(
        first.physical, second.physical,
        "seed={seed:#010x}: physical traces differ on replay"
    );
}

/// Block 1 holds the two seeding operations; later blocks cycle through
/// `sizes`.
pub fn block_size(block: u64, sizes: &[usize]) -> usize {
    if block == 1 {
        2
    } else {
        sizes[usize::try_from(block - 2).expect("small") % sizes.len()]
    }
}

/// An operation budget around the operation's full `cost`: mostly exact,
/// else one unit short, unlimited, zero or roomy.
pub fn draw_budget(generator: &mut Generator, cost: u64) -> Budget {
    match generator.index(8) {
        0..=3 => Budget::limit(cost),
        4 => Budget::limit(cost.saturating_sub(1)),
        5 => Budget::unlimited(),
        6 => Budget::limit(0),
        _ => Budget::limit(cost + 5),
    }
}

/// Publishes `schedule` as a snapshot of its own on the head, whose schedule
/// id is `current`, and returns that snapshot. On the way it checks that a
/// stale id is refused, that a pricing change inside a rolled-back span does
/// not publish, and that the candidate stays pinned to its base's schedule.
pub fn publish_pricing_candidate(
    engine: &mut Engine,
    current: u32,
    schedule: &PriceSchedule,
    next_snapshot: &mut u64,
    ctx: &Ctx,
) -> Snapshot {
    let mut admin = engine.begin(engine.head(), None).unwrap();
    assert_eq!(admin.pricing().id(), current, "{ctx}: pinned to the base");
    assert_eq!(
        admin.set_pricing(PriceSchedule::uniform(current, 9)),
        Err(EngineError::StaleSchedule { current }),
        "{ctx}: a schedule id must exceed the committed one"
    );
    // The second set_pricing succeeds only if the rollback restored the
    // base id.
    let checkpoint = admin.checkpoint();
    admin
        .set_pricing(schedule.clone().with_id(schedule.id() + 100))
        .unwrap();
    assert_eq!(admin.rollback_to(checkpoint), Ok(0..0), "{ctx}");
    admin
        .set_pricing(schedule.clone())
        .unwrap_or_else(|error| panic!("{ctx}: rolled-back pricing leaked: {error}"));
    assert_eq!(
        admin.pricing().id(),
        current,
        "{ctx}: pinned pricing never changes"
    );
    let sealed = admin.seal();
    assert!(sealed.receipts().is_empty(), "{ctx}");
    assert_eq!(sealed.spent(), Cost::ZERO, "{ctx}");
    let published = engine.publish(&sealed).unwrap();
    assert_eq!(
        published.sequence(),
        *next_snapshot,
        "{ctx}: snapshot ids are sequential"
    );
    *next_snapshot += 1;
    let head = engine.head_snapshot();
    assert_eq!(head.id(), published, "{ctx}");
    assert_eq!(
        head.pricing(),
        schedule,
        "{ctx}: the admin snapshot commits the schedule"
    );
    head
}

/// Seals the candidate: its receipts and total are the oracle's, whatever
/// was rolled back.
pub fn seal_checked(
    branch: Branch,
    base: SnapshotId,
    receipts: &[Receipt],
    spent: u64,
    ctx: &Ctx,
) -> SealedCandidate {
    assert_eq!(branch.receipts(), receipts, "{ctx}");
    assert_eq!(branch.spent(), Cost::new(spent), "{ctx}");
    let sealed = branch.seal();
    assert_eq!(sealed.base(), base, "{ctx}");
    assert_eq!(sealed.receipts(), receipts, "{ctx}");
    assert_eq!(sealed.spent(), Cost::new(spent), "{ctx}");
    sealed
}

/// Publishes the block's candidate, then offers the loser sealed on the
/// same base, which must be refused and change nothing. Returns the new
/// head and the loser's refusal.
pub fn publish_checked(
    engine: &mut Engine,
    sealed: &SealedCandidate,
    loser: Option<&SealedCandidate>,
    next_snapshot: &mut u64,
    ctx: &Ctx,
) -> (SnapshotId, Option<EngineError>) {
    let base = sealed.base();
    let id = engine.publish(sealed).unwrap();
    assert_eq!(
        id.sequence(),
        *next_snapshot,
        "{ctx}: snapshot ids are sequential"
    );
    *next_snapshot += 1;
    let stale = EngineError::StaleHead {
        expected: base,
        head: id,
    };
    let refusal = loser.map(|loser| {
        let error = engine
            .publish(loser)
            .expect_err("a candidate whose base is no longer head cannot publish");
        assert_eq!(error, stale, "{ctx}");
        assert_eq!(engine.head(), id, "{ctx}: a stale publish changes nothing");
        error
    });
    assert_eq!(
        engine.begin(base, None).err(),
        Some(stale),
        "{ctx}: the old head is no longer a valid base"
    );
    assert_eq!(
        engine.snapshot_count(),
        usize::try_from(*next_snapshot).expect("small"),
        "{ctx}"
    );
    (id, refusal)
}

/// A history test and a replay test per seed. Expects `full_run(seed)` and
/// `replay(seed)` where it is invoked.
macro_rules! seed_tests {
    ($($history:ident, $replay:ident => $seed:expr;)*) => {
        $(
            #[test]
            fn $history() {
                full_run($seed);
            }

            #[test]
            fn $replay() {
                replay($seed);
            }
        )*
    };
}
pub(crate) use seed_tests;
