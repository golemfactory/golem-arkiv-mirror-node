//! Cross-namespace isolation and directory bounds.
//!
//! Namespace A (64 records) runs one fixed workload of traced reads and writes
//! on a branch from the current head; the branch is dropped, never sealed or
//! published, so A's published state is identical in every phase. Between the
//! phases another namespace (B) grows by 10 000 records and the directory
//! grows by 500 namespaces.
//!
//! Receipts and results are hand-computed literals; the physical node counts
//! are derived by hand from the shape of A's raw record trie and of the
//! directory trie (see the comment above `A_TRIE_NODES`). Both must be the
//! same in every phase, and a snapshot pinned after phase 0 must keep its
//! answers. State roots are covered by `roots.rs`.

use std::collections::BTreeSet;

use arkiv_db::{
    Branch, Budget, Cost, Effect, Engine, Entry, NamespaceDescriptor, NamespaceId, OperationClass,
    Outcome, PriceSchedule, Receipt, Rejection, Revision, Snapshot, SnapshotId,
};

use crate::support::{
    isolation::{
        DIRECTORY_PATH_BOUND, Expected, Step, assert_directory_nodes_equal,
        assert_directory_nodes_follow_the_trie, assert_matches_expected,
        assert_namespace_local_nodes_equal, assert_receipts_and_results_equal, assert_steps_repeat,
        expected_receipt, find, publish_block, read_step, write_step,
    },
    reader::Reader,
    vocab::{
        ALICE, BOB, CAROL, DAVE, add_namespace, applied, apply, begin, delete, descriptor, entry,
        ledger, put, read_applied, read_rejected, rejected, traced, transfer,
    },
};

/// A holds `a/00`..`a/59` (4-byte keys) with values `text-00`..`text-59`
/// (7 bytes each) plus the four binary records below: 64 records in total.
const TEXT_RECORDS: usize = 60;
const BINARY_RECORDS: [(&[u8], &[u8]); 4] = [
    (b"\x00", b"nul"),                      // 1-byte key, 3-byte value
    (b"a\x00", b""),                        // 2-byte key, empty value
    (b"a\xff", b"high"),                    // 2-byte key, 4-byte value
    (b"\xff\xff", b"\xff\xff\xff\xff\xff"), // 2-byte key, 5-byte value
];
const A_RECORDS: usize = 64;

/// B holds `b/00000`..`b/00004` after phase 0 (7-byte keys, 9-byte values
/// `bee-NNNNN`) and `b/00005`..`b/10004` on top after phase 1.
const B_INITIAL_RECORDS: usize = 5;
const B_GROWTH_RECORDS: usize = 10_000;
const B_GROWTH_BLOCKS: usize = 5;
const B_TOTAL_RECORDS: usize = B_INITIAL_RECORDS + B_GROWTH_RECORDS;
/// A key that exists in B in every phase and never in A.
const B_ONLY_KEY: &[u8] = b"b/00000";

/// Namespaces created in phase 2, on top of A, B and C.
const EXTRA_NAMESPACES: usize = 500;
const PHASE0_NAMESPACES: usize = 3;
const PHASE2_NAMESPACES: usize = PHASE0_NAMESPACES + EXTRA_NAMESPACES;

// The costs below are hand-computed from the FIXTURE unit prices (see
// `support/pricing.rs`); RootUpdate is charged once per create/transfer group
// and twice per put/delete group.

/// get present: DirectoryLookup 1 + PointRead 3 + ReadByte 7 (`text-07`).
const GET_PRESENT_COST: u64 = 11;
/// get missing: DirectoryLookup 1 + PointRead 3.
const GET_MISS_COST: u64 = 4;
/// descriptor: DirectoryLookup 1.
const DESCRIPTOR_COST: u64 = 1;
/// scan "" over all 64 records: 1 + 4 + per entry (2 + key + value bytes):
/// 60 text entries at 2 + 4 + 7 = 13 -> 780; binary entries 2+1+3 = 6,
/// 2+2+0 = 4, 2+2+4 = 8, 2+2+5 = 9 -> 27; total 5 + 780 + 27 = 812.
const SCAN_ALL_COST: u64 = 812;
/// Key and value bytes of all 64 records: 60 * 11 + (4 + 2 + 6 + 7) = 679.
const SCAN_ALL_BYTES: u64 = 679;
/// scan "a/1": `a/10`..`a/19`, 10 entries at 13 -> 5 + 130 = 135.
const SCAN_A1_COST: u64 = 135;
/// scan of a prefix with no entry: DirectoryLookup 1 + ScanSeek 4.
const SCAN_MISS_COST: u64 = 5;
/// The first three entries of A in byte order are `\x00` -> `nul` (6),
/// `a\x00` -> `` (4) and `a/00` -> `text-00` (13); 5 + 6 + 4 + 13 = 28. The
/// fourth entry `a/01` (13) would need 41, so the scan stops with 28 spent.
const ABORTED_SCAN_LIMIT: u64 = 28;
/// Key and value bytes of those three entries: 4 + 2 + 11 = 17.
const ABORTED_SCAN_BYTES: u64 = 17;
/// put `a/00` = `replaced`: 1 + 10 + 2 * 4 + 8 + RootUpdate 2 * 2.
const PUT_EXISTING_COST: u64 = 31;
/// put `a/new` = `fresh`: 1 + 10 + 2 * 5 + 5 + RootUpdate 2 * 2.
const PUT_NEW_COST: u64 = 30;
/// delete `a/01`: 1 + 5 + 2 * 4 + RootUpdate 2 * 2.
const DELETE_EXISTING_COST: u64 = 18;
/// delete `zzz` (absent): 1 + 5 + 2 * 3 + RootUpdate 2 * 2.
const DELETE_MISSING_COST: u64 = 16;
/// transfer: DirectoryLookup 1 + NamespaceTransfer 20 + RootUpdate 2.
const TRANSFER_COST: u64 = 23;
/// put by a former owner: DirectoryLookup 1, then Unauthorized.
const UNAUTHORIZED_COST: u64 = 1;
/// Phase 0 block: 3 creates at 1 + 50 + 2 = 53 -> 159; 60 text puts at
/// 1 + 10 + 8 + 7 + 4 = 30 -> 1800; binary puts 20 + 19 + 23 + 24 = 86; 5 B
/// puts at 1 + 10 + 14 + 9 + 4 = 38 -> 190. Total 159 + 1800 + 86 + 190 =
/// 2235.
const PHASE0_BLOCK_COST: u64 = 2235;
/// One growth block: 2000 B puts at 38.
const B_GROWTH_BLOCK_COST: u64 = 76_000;
/// Phase 2 block: 500 creates at 53.
const PHASE2_BLOCK_COST: u64 = 26_500;
/// scan "b/" over B after phase 1: 5 + 10 005 entries at 2 + 7 + 9 = 18.
const B_SCAN_ALL_COST: u64 = 180_095;

const GET_PRESENT: &str = "get present key a/07";
const GET_ABSENT: &str = "get absent key a/99";
const GET_FOREIGN_KEY: &str = "get key present only in B";
const DESCRIPTOR: &str = "descriptor";
const SCAN_ALL: &str = "scan_prefix(\"\")";
const SCAN_A1: &str = "scan_prefix(\"a/1\")";
const SCAN_FOREIGN_PREFIX: &str = "scan_prefix(\"b/\")";
const SCAN_ABORTED: &str = "scan_prefix(\"\") with an exact three-entry budget";
const PUT_EXISTING: &str = "put existing key";
const PUT_NEW: &str = "put new key";
const DELETE_EXISTING: &str = "delete existing key";
const DELETE_MISSING: &str = "delete missing key";
const TRANSFER: &str = "transfer alice -> bob";
const PUT_AFTER_TRANSFER: &str = "put by alice after the transfer";

/// The logical result of one workload step.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Observed {
    Value(Option<Vec<u8>>),
    Entries(Vec<Entry>),
    Descriptor(NamespaceDescriptor),
    Applied(Effect),
    Rejected(Rejection),
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

/// The read part of A's workload, in order.
fn read_workload(reader: &impl Reader, a: NamespaceId) -> Vec<Step<Observed>> {
    let traced = Budget::unlimited().traced();
    vec![
        read_step(GET_PRESENT, reader.get(a, b"a/07", traced), Observed::Value),
        read_step(GET_ABSENT, reader.get(a, b"a/99", traced), Observed::Value),
        read_step(
            GET_FOREIGN_KEY,
            reader.get(a, B_ONLY_KEY, traced),
            Observed::Value,
        ),
        read_step(
            DESCRIPTOR,
            reader.descriptor(a, traced),
            Observed::Descriptor,
        ),
        read_step(
            SCAN_ALL,
            reader.scan_prefix(a, b"", traced),
            Observed::Entries,
        ),
        read_step(
            SCAN_A1,
            reader.scan_prefix(a, b"a/1", traced),
            Observed::Entries,
        ),
        read_step(
            SCAN_FOREIGN_PREFIX,
            reader.scan_prefix(a, b"b/", traced),
            Observed::Entries,
        ),
        read_step(
            SCAN_ABORTED,
            reader.scan_prefix(a, b"", Budget::limit(ABORTED_SCAN_LIMIT).traced()),
            Observed::Entries,
        ),
    ]
}

/// The write part of A's workload, in order, on a branch that is dropped.
fn write_workload(branch: &mut Branch, a: NamespaceId) -> Vec<Step<Observed>> {
    let traced = Budget::unlimited().traced();
    vec![
        write_step(
            PUT_EXISTING,
            branch.execute(ALICE, put(a, b"a/00", b"replaced"), traced),
        ),
        write_step(
            PUT_NEW,
            branch.execute(ALICE, put(a, b"a/new", b"fresh"), traced),
        ),
        write_step(
            DELETE_EXISTING,
            branch.execute(ALICE, delete(a, b"a/01"), traced),
        ),
        write_step(
            DELETE_MISSING,
            branch.execute(ALICE, delete(a, b"zzz"), traced),
        ),
        write_step(TRANSFER, branch.execute(ALICE, transfer(a, 0, BOB), traced)),
        write_step(
            PUT_AFTER_TRANSFER,
            branch.execute(ALICE, put(a, b"a/02", b"nope"), traced),
        ),
    ]
}

/// Runs the whole workload on a branch from the current head and drops the
/// branch, so the published state is exactly what it was before.
fn run_workload(world: &World) -> Vec<Step<Observed>> {
    let head = world.engine.head();
    let mut branch = begin(&world.engine, None);
    let mut steps = read_workload(&branch, world.a);
    steps.extend(write_workload(&mut branch, world.a));
    drop(branch);
    assert_eq!(
        world.engine.head(),
        head,
        "a dropped branch never publishes"
    );
    steps
}

/// The hand-computed baseline: every receipt and result of the workload.
fn expected_baseline(a: NamespaceId) -> Vec<Expected<Observed>> {
    use OperationClass::{
        DeleteRecord, DirectoryLookup, PointRead, PutRecord, ReadByte, RootUpdate, ScanEntry,
        ScanSeek, WriteKeyByte, WriteValueByte,
    };
    let miss_ledger = [(DirectoryLookup, 1, 1, 1), (PointRead, 1, 3, 3)];
    let expected = |name, observed, receipt| Expected {
        name,
        observed,
        receipts: vec![receipt],
    };
    vec![
        expected(
            GET_PRESENT,
            Observed::Value(Some(b"text-07".to_vec())),
            traced(
                read_applied(GET_PRESENT_COST),
                ledger(&[
                    (DirectoryLookup, 1, 1, 1),
                    (PointRead, 1, 3, 3),
                    (ReadByte, 7, 1, 7),
                ]),
            ),
        ),
        expected(
            GET_ABSENT,
            Observed::Value(None),
            traced(read_applied(GET_MISS_COST), ledger(&miss_ledger)),
        ),
        expected(
            GET_FOREIGN_KEY,
            Observed::Value(None),
            traced(read_applied(GET_MISS_COST), ledger(&miss_ledger)),
        ),
        expected(
            DESCRIPTOR,
            Observed::Descriptor(descriptor(a, ALICE, 0)),
            traced(
                read_applied(DESCRIPTOR_COST),
                ledger(&[(DirectoryLookup, 1, 1, 1)]),
            ),
        ),
        expected(
            SCAN_ALL,
            Observed::Entries(a_records_in_key_order()),
            traced(
                read_applied(SCAN_ALL_COST),
                ledger(&[
                    (DirectoryLookup, 1, 1, 1),
                    (ScanSeek, 1, 4, 4),
                    (ScanEntry, 64, 2, 128),
                    (ReadByte, SCAN_ALL_BYTES, 1, SCAN_ALL_BYTES),
                ]),
            ),
        ),
        expected(
            SCAN_A1,
            Observed::Entries(
                a_records_in_key_order()
                    .into_iter()
                    .filter(|entry| entry.key.starts_with(b"a/1"))
                    .collect(),
            ),
            traced(
                read_applied(SCAN_A1_COST),
                ledger(&[
                    (DirectoryLookup, 1, 1, 1),
                    (ScanSeek, 1, 4, 4),
                    (ScanEntry, 10, 2, 20),
                    (ReadByte, 110, 1, 110),
                ]),
            ),
        ),
        expected(
            SCAN_FOREIGN_PREFIX,
            Observed::Entries(Vec::new()),
            traced(
                read_applied(SCAN_MISS_COST),
                ledger(&[(DirectoryLookup, 1, 1, 1), (ScanSeek, 1, 4, 4)]),
            ),
        ),
        expected(
            SCAN_ABORTED,
            Observed::Rejected(Rejection::OutOfBudget),
            traced(
                read_rejected(ABORTED_SCAN_LIMIT, Rejection::OutOfBudget),
                ledger(&[
                    (DirectoryLookup, 1, 1, 1),
                    (ScanSeek, 1, 4, 4),
                    (ScanEntry, 3, 2, 6),
                    (ReadByte, ABORTED_SCAN_BYTES, 1, ABORTED_SCAN_BYTES),
                ]),
            ),
        ),
        expected(
            PUT_EXISTING,
            Observed::Applied(Effect::Put { replaced: true }),
            traced(
                applied(PUT_EXISTING_COST),
                ledger(&[
                    (DirectoryLookup, 1, 1, 1),
                    (PutRecord, 1, 10, 10),
                    (WriteKeyByte, 4, 2, 8),
                    (WriteValueByte, 8, 1, 8),
                    (RootUpdate, 2, 2, 4),
                ]),
            ),
        ),
        expected(
            PUT_NEW,
            Observed::Applied(Effect::Put { replaced: false }),
            traced(
                applied(PUT_NEW_COST),
                ledger(&[
                    (DirectoryLookup, 1, 1, 1),
                    (PutRecord, 1, 10, 10),
                    (WriteKeyByte, 5, 2, 10),
                    (WriteValueByte, 5, 1, 5),
                    (RootUpdate, 2, 2, 4),
                ]),
            ),
        ),
        expected(
            DELETE_EXISTING,
            Observed::Applied(Effect::Deleted { existed: true }),
            traced(
                applied(DELETE_EXISTING_COST),
                ledger(&[
                    (DirectoryLookup, 1, 1, 1),
                    (DeleteRecord, 1, 5, 5),
                    (WriteKeyByte, 4, 2, 8),
                    (RootUpdate, 2, 2, 4),
                ]),
            ),
        ),
        expected(
            DELETE_MISSING,
            Observed::Applied(Effect::Deleted { existed: false }),
            traced(
                applied(DELETE_MISSING_COST),
                ledger(&[
                    (DirectoryLookup, 1, 1, 1),
                    (DeleteRecord, 1, 5, 5),
                    (WriteKeyByte, 3, 2, 6),
                    (RootUpdate, 2, 2, 4),
                ]),
            ),
        ),
        expected(
            TRANSFER,
            Observed::Applied(Effect::Transferred {
                revision: Revision(1),
            }),
            transfer_receipt(),
        ),
        expected(
            PUT_AFTER_TRANSFER,
            Observed::Rejected(Rejection::Unauthorized),
            traced(
                rejected(UNAUTHORIZED_COST, Rejection::Unauthorized),
                ledger(&[(DirectoryLookup, 1, 1, 1)]),
            ),
        ),
    ]
}

/// The receipt of every successful transfer under the fixture schedule.
fn transfer_receipt() -> Receipt {
    traced(
        applied(TRANSFER_COST),
        ledger(&[
            (OperationClass::DirectoryLookup, 1, 1, 1),
            (OperationClass::NamespaceTransfer, 1, 20, 20),
            (OperationClass::RootUpdate, 1, 2, 2),
        ]),
    )
}

// Every map is a Merkle Patricia trie (see docs/state-trie.md). A node visit
// is counted once per step that examines the node: a lookup or an update
// counts the key's path, a removal also the sibling it loads when a branch
// collapses, and a scan its seek path plus every node it steps into.
//
// The directory is keyed by 8-byte big-endian ids (16 nibbles). With A, B
// and C (ids 1, 2, 3) the ids share their first 15 nibbles, so the trie is
// an extension over 15 zero nibbles, a branch on the last nibble and three
// leaves: a lookup of A visits 3 nodes. With the 500 extra namespaces of
// phase 2 (ids 1 to 503 = 0x001 to 0x1f7) the ids share 13 zero nibbles
// (an extension), nibble 13 is 0 or 1 (a branch), ids 0x001 to 0x0ff span
// every value of nibble 14 (a branch) and 0x001 to 0x00f every nonzero
// value of nibble 15 (a branch), so A's lookup visits the extension, three
// branches and its leaf: 5 nodes. A read resolves A once; a write resolves
// it and then inserts the updated catalog leaf along the same path, twice
// the nodes; a write rejected as unauthorized stops after the lookup.
//
// A's raw record map: 64 keys, each encoded as its bytes with `00` escaped
// to `01 01` and `01` to `01 02`, then a `00` terminator. `\x00` (`01 01 00`)
// starts with nibble 0, the `a…` keys with 6 (0x61), `\xff\xff` with f, so
// the root is a branch with three children.
// Under slot 6 every key continues with nibble 1 (an extension of one
// nibble), then the next byte's high nibble is 0 (`a\x00`, escaped), f (`a\xff`) or
// 2 (`a/…`, 0x2f): a branch B2 with two leaves. The 60 `a/XY` keys share
// nibbles f and 3 (0x2f's low nibble, the digit byte's high nibble): an
// extension of two nibbles, then a branch B5 on the tens digit X (slots 0
// to 5); each tens group shares nibble 3 again (an extension of one nibble)
// and branches on the units digit Y (slots 0 to 9) into ten leaves. Nodes:
// the root, the `\x00` and `\xff\xff` leaves, the one-nibble extension, B2,
// its two leaves, the two-nibble extension, B5, and per tens digit an
// extension, a branch and ten leaves: 9 + 6 × 12 = 81.

/// Directory nodes on A's lookup path with A, B and C only.
const DIRECTORY_PATH_THREE: u64 = 3;
/// Directory nodes on A's lookup path with 503 namespaces.
const DIRECTORY_PATH_503: u64 = 5;
/// Every node of A's raw record map.
const A_TRIE_NODES: u64 = 81;

const _: () = assert!(DIRECTORY_PATH_THREE <= DIRECTORY_PATH_BOUND);
const _: () = assert!(DIRECTORY_PATH_503 <= DIRECTORY_PATH_BOUND);

/// A's record-node counts, derived from the shape of its raw map above.
/// They depend only on A's own map and hold in every phase.
fn assert_absolute_record_bounds(steps: &[Step<Observed>], phase: &str) {
    let expected: [(&str, u64); 14] = [
        // Root, one-nibble extension, B2 (slot 2), two-nibble extension,
        // B5 (slot 0), extension, the tens-0 branch (slot 7), the leaf.
        (GET_PRESENT, 8),
        // `a/99` follows `a/07`'s path to B5, whose slot 9 is empty.
        (GET_ABSENT, 5),
        // `b/…` (0x62) takes slot 6 and leaves at the extension over 1.
        (GET_FOREIGN_KEY, 2),
        (DESCRIPTOR, 0),
        // The seek descends the root to the `\x00` leaf; the walk then
        // steps into every other node once.
        (SCAN_ALL, A_TRIE_NODES),
        // Seek for `a/1`: root, extension, B2, extension, B5 (slot 1), and
        // the tens-1 extension, whose path [3] is above the 0 nibble of the
        // prefix's terminator, so it is the first pending node (6).
        // Walking it: the tens-1 branch and its ten leaves (11); the step
        // that ends the scan: the tens-2 extension, branch and `a/20` (3).
        (SCAN_A1, 6 + 11 + 3),
        // Seek for `b/`: root (slot 6) and the extension over 1, which is
        // below `b` (0x62): nothing pending there; the next step takes the
        // root's slot f, the `\xff\xff` leaf, which ends the scan.
        (SCAN_FOREIGN_PREFIX, 3),
        // Seek (root, `\x00`); `a\x00` (extension, B2, leaf); `a/00` (the
        // two-nibble extension, B5, extension, branch, leaf); the step that
        // reveals `a/01`, whose charge fails (its leaf): 2 + 3 + 5 + 1.
        (SCAN_ABORTED, 11),
        // The update path of `a/00`, as for the lookup of `a/07`.
        (PUT_EXISTING, 8),
        // `a/new` (…2f 6e…) leaves the two-nibble extension [f, 3] after
        // its first nibble: root, extension, B2, the two-nibble extension.
        // The split keeps B5 below a new branch without visiting it.
        (PUT_NEW, 4),
        // `a/01` after the split: root, extension, B2, the new one-nibble
        // extension, the new branch (slot 3), B5, extension, the tens-0
        // branch, the leaf. Nine children stay, so nothing collapses.
        (DELETE_EXISTING, 9),
        // `zzz` (0x7a) finds the root's slot 7 empty.
        (DELETE_MISSING, 1),
        (TRANSFER, 0),
        (PUT_AFTER_TRANSFER, 0),
    ];
    for (name, nodes) in expected {
        assert_eq!(
            find(steps, name).physical().record_nodes,
            nodes,
            "{phase}: record nodes of {name}"
        );
    }
    let miss = find(steps, SCAN_FOREIGN_PREFIX);
    assert_eq!(
        miss.receipt().spent,
        Cost::new(SCAN_MISS_COST),
        "{phase}: miss cost"
    );
    assert_eq!(
        miss.observed,
        Observed::Entries(Vec::new()),
        "{phase}: miss result"
    );
    // The aborted scan has charged exactly the first three entries and
    // stepped only as far as the fourth: it stops as soon as the budget is
    // exhausted rather than materializing the whole result.
    let aborted = find(steps, SCAN_ABORTED);
    assert_eq!(
        aborted.receipt().spent,
        Cost::new(ABORTED_SCAN_LIMIT),
        "{phase}: aborted scan spent"
    );
    assert_eq!(
        aborted.observed,
        Observed::Rejected(Rejection::OutOfBudget),
        "{phase}: aborted scan outcome"
    );
}

/// A's 64 records in fixture (insertion) order.
fn a_records() -> Vec<Entry> {
    let mut records: Vec<Entry> = (0..TEXT_RECORDS)
        .map(|i| {
            entry(
                format!("a/{i:02}").as_bytes(),
                format!("text-{i:02}").as_bytes(),
            )
        })
        .collect();
    records.extend(BINARY_RECORDS.iter().map(|&(key, value)| entry(key, value)));
    records
}

/// A's records as an unsigned byte-ordered scan must return them.
fn a_records_in_key_order() -> Vec<Entry> {
    let mut records = a_records();
    records.sort_by(|left, right| left.key.cmp(&right.key));
    records
}

fn b_key(i: usize) -> Vec<u8> {
    format!("b/{i:05}").into_bytes()
}

fn b_value(i: usize) -> Vec<u8> {
    format!("bee-{i:05}").into_bytes()
}

/// The engine with namespaces A (alice), B (bob) and C (carol).
struct World {
    engine: Engine,
    a: NamespaceId,
    b: NamespaceId,
    c: NamespaceId,
    /// The snapshot published at the end of phase 0.
    phase0: SnapshotId,
}

impl World {
    /// Phase 0: one block creates A, B and C, fills A with 64 records and B
    /// with 5; C stays empty.
    fn publish_phase0() -> Self {
        let mut engine = Engine::new(PriceSchedule::FIXTURE);
        let mut branch = begin(&engine, None);
        let a = add_namespace(&mut branch, ALICE, "test");
        let b = add_namespace(&mut branch, BOB, "test");
        let c = add_namespace(&mut branch, CAROL, "test");
        for record in a_records() {
            let executed = apply(&mut branch, ALICE, put(a, &record.key, &record.value));
            assert_eq!(
                executed.effect,
                Effect::Put { replaced: false },
                "fixture keys are distinct"
            );
        }
        for i in 0..B_INITIAL_RECORDS {
            apply(&mut branch, BOB, put(b, &b_key(i), &b_value(i)));
        }
        let phase0 = publish_block(&mut engine, branch, PHASE0_BLOCK_COST, "phase 0 block");
        let world = Self {
            engine,
            a,
            b,
            c,
            phase0,
        };
        let head = world.head();
        assert_eq!(head.id(), phase0);
        assert_eq!(head.namespace_count(), PHASE0_NAMESPACES);
        assert_eq!(head.record_count(a), Some(A_RECORDS));
        assert_eq!(head.record_count(b), Some(B_INITIAL_RECORDS));
        assert_eq!(head.record_count(c), Some(0));
        world
    }

    /// Phase 1: 10 000 more records into B across five blocks; no other
    /// namespace is touched and none is created.
    fn grow_b(&mut self) {
        let per_block = B_GROWTH_RECORDS / B_GROWTH_BLOCKS;
        for block in 0..B_GROWTH_BLOCKS {
            let mut branch = begin(&self.engine, None);
            let first = B_INITIAL_RECORDS + block * per_block;
            for i in first..first + per_block {
                let executed = apply(&mut branch, BOB, put(self.b, &b_key(i), &b_value(i)));
                assert_eq!(
                    executed.effect,
                    Effect::Put { replaced: false },
                    "growth keys are new"
                );
            }
            publish_block(
                &mut self.engine,
                branch,
                B_GROWTH_BLOCK_COST,
                &format!("growth block {block}"),
            );
        }
        assert_eq!(self.head().record_count(self.b), Some(B_TOTAL_RECORDS));
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

    fn descriptor_at_head(&self, namespace: NamespaceId) -> NamespaceDescriptor {
        self.head()
            .descriptor(namespace, Budget::unlimited())
            .expect("namespace exists")
            .value
    }

    /// The precondition between phases: A's record map is physically the same
    /// object and its descriptor is unchanged.
    fn assert_a_untouched_since(&self, before: &Snapshot, phase: &str) {
        let after = self.head();
        assert!(
            before.records_shared_with(&after, self.a),
            "{phase}: A's record map must be shared with the earlier head"
        );
        assert_eq!(
            self.descriptor_at_head(self.a),
            descriptor(self.a, ALICE, 0),
            "{phase}: A's descriptor"
        );
        assert_eq!(
            after.record_count(self.a),
            Some(A_RECORDS),
            "{phase}: A size"
        );
    }
}

#[test]
fn workload_baseline_matches_hand_computed_receipts() {
    let world = World::publish_phase0();
    let before = world.head();
    let baseline = run_workload(&world);
    assert_matches_expected(&baseline, &expected_baseline(world.a), "phase 0");
    assert_absolute_record_bounds(&baseline, "phase 0");
    assert_directory_nodes_follow_the_trie(&baseline, DIRECTORY_PATH_THREE, "phase 0");
    // The dropped branch changed nothing: not the records, not the owner.
    world.assert_a_untouched_since(&before, "phase 0 after the discarded branch");
    assert_eq!(world.head().id(), world.phase0);
    assert_eq!(world.engine.snapshot_count(), 2, "genesis and phase 0 only");
}

#[test]
fn workload_is_unchanged_by_growth_of_another_namespace() {
    let mut world = World::publish_phase0();
    let baseline = run_workload(&world);
    let before = world.head();

    world.grow_b();

    let after = world.head();
    assert_eq!(
        after.namespace_count(),
        PHASE0_NAMESPACES,
        "phase 1 creates nothing"
    );
    assert!(
        !before.records_shared_with(&after, world.b),
        "B's record map did change"
    );
    world.assert_a_untouched_since(&before, "phase 1");

    let rerun = run_workload(&world);
    assert_receipts_and_results_equal(&baseline, &rerun, "phase 1");
    assert_namespace_local_nodes_equal(&baseline, &rerun, "phase 1");
    // Replacing B's records replaces the directory node's value only; the
    // directory keeps its shape, so A's lookup and update paths are the same.
    assert_directory_nodes_equal(&baseline, &rerun, "phase 1");
    assert_absolute_record_bounds(&rerun, "phase 1");
    assert_directory_nodes_follow_the_trie(&rerun, DIRECTORY_PATH_THREE, "phase 1");
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
        after.record_count(world.b),
        Some(B_TOTAL_RECORDS),
        "phase 2 writes nothing"
    );
    assert!(
        before.records_shared_with(&after, world.b),
        "B's record map is untouched by namespace creation"
    );
    world.assert_a_untouched_since(&before, "phase 2");

    let rerun = run_workload(&world);
    assert_receipts_and_results_equal(&baseline, &rerun, "phase 2");
    assert_namespace_local_nodes_equal(&baseline, &rerun, "phase 2");
    // The directory did grow, so A's lookup path is longer: the shape for
    // 503 ids derived above, within the 17-node bound of 8-byte keys.
    assert_directory_nodes_follow_the_trie(&rerun, DIRECTORY_PATH_503, "phase 2");
    assert_absolute_record_bounds(&rerun, "phase 2");
    world.assert_a_untouched_since(&before, "phase 2 after the discarded branch");
}

#[test]
fn historical_snapshot_keeps_its_answers_and_receipts_after_later_phases() {
    let mut world = World::publish_phase0();
    let baseline = run_workload(&world);
    world.grow_b();
    world.create_namespaces();

    let historical = world
        .engine
        .snapshot(world.phase0)
        .expect("phase 0 is retained");
    assert_eq!(historical.namespace_count(), PHASE0_NAMESPACES);
    assert_eq!(historical.record_count(world.b), Some(B_INITIAL_RECORDS));
    let reread = read_workload(&historical, world.a);
    assert_eq!(reread.len(), 8, "the read part of the workload");
    // The pinned state is the very state the baseline branch was cloned
    // from, so even the physical layout is the same.
    assert_steps_repeat(&reread, &baseline, "historical");
    assert_absolute_record_bounds(&run_workload(&world), "head after phase 2");
    // The pinned snapshot still shares A's map with the current head.
    assert!(historical.records_shared_with(&world.head(), world.a));
}

#[test]
fn transfer_work_is_independent_of_namespace_size() {
    let mut world = World::publish_phase0();
    world.grow_b();
    let before = world.head();
    let traced = Budget::unlimited().traced();

    // One block transfers the small namespace and the large one: the owners
    // change and the record maps of both namespaces are still the same
    // physical objects as before.
    let mut branch = begin(&world.engine, None);
    let small = branch
        .execute(ALICE, transfer(world.a, 0, BOB), traced)
        .expect("alice owns A");
    let large = branch
        .execute(BOB, transfer(world.b, 0, CAROL), traced)
        .expect("bob owns B");
    assert_eq!(small.receipt, transfer_receipt(), "transfer of 64 records");
    assert_eq!(
        large.receipt,
        transfer_receipt(),
        "transfer of 10 005 records"
    );
    let transferred = Effect::Transferred {
        revision: Revision(1),
    };
    assert_eq!(small.effect, transferred);
    assert_eq!(large.effect, transferred);
    assert_eq!(
        small.physical.record_nodes, 0,
        "A's records are not visited"
    );
    assert_eq!(
        large.physical.record_nodes, 0,
        "B's records are not visited"
    );
    publish_block(
        &mut world.engine,
        branch,
        2 * TRANSFER_COST,
        "transfer block",
    );
    let after = world.head();
    assert!(
        before.records_shared_with(&after, world.a),
        "A moved no record"
    );
    assert!(
        before.records_shared_with(&after, world.b),
        "B moved no record"
    );
    assert_eq!(after.record_count(world.a), Some(A_RECORDS));
    assert_eq!(after.record_count(world.b), Some(B_TOTAL_RECORDS));
    assert_eq!(
        world.descriptor_at_head(world.a),
        descriptor(world.a, BOB, 1)
    );
    assert_eq!(
        world.descriptor_at_head(world.b),
        descriptor(world.b, CAROL, 1)
    );

    // Reads through the transferred namespaces are the same reads as before.
    let key = b_key(B_TOTAL_RECORDS - 1);
    let read_before = before.get(world.b, &key, traced).expect("B exists");
    let read_after = after.get(world.b, &key, traced).expect("B exists");
    assert_eq!(read_after.value, Some(b_value(B_TOTAL_RECORDS - 1)));
    assert_eq!(read_after.value, read_before.value);
    assert_eq!(read_after.receipt, read_before.receipt);
    assert_eq!(
        read_after.physical.record_nodes,
        read_before.physical.record_nodes
    );
    let scan_before = before
        .scan_prefix(world.a, b"a/1", traced)
        .expect("A exists");
    let scan_after = after
        .scan_prefix(world.a, b"a/1", traced)
        .expect("A exists");
    assert_eq!(scan_after.value, scan_before.value);
    assert_eq!(scan_after.receipt, scan_before.receipt);
    assert_eq!(scan_after.receipt.spent, Cost::new(SCAN_A1_COST));
}

#[test]
fn keys_of_another_namespace_are_invisible_and_miss_at_fixed_cost() {
    let mut world = World::publish_phase0();
    world.grow_b();
    let head = world.head();
    let traced = Budget::unlimited().traced();
    let expected = expected_baseline(world.a);
    let miss_receipt = |name| expected_receipt(&expected, name);

    // Nothing A returns is a key of B, in any read shape.
    let b_keys: BTreeSet<Vec<u8>> = (0..B_TOTAL_RECORDS).map(b_key).collect();
    let a_all = head.scan_prefix(world.a, b"", traced).expect("A exists");
    assert_eq!(a_all.value, a_records_in_key_order());
    assert!(
        a_all.value.iter().all(|entry| !b_keys.contains(&entry.key)),
        "A's scan must not contain a key of B"
    );
    assert_eq!(a_all.receipt.spent, Cost::new(SCAN_ALL_COST));
    for key in [b"b/00000".as_slice(), b"b/00004", b"b/05000", b"b/10004"] {
        let in_b = head.get(world.b, key, traced).expect("B exists");
        assert!(in_b.value.is_some(), "{key:?} is present in B");
        let through_a = head.get(world.a, key, traced).expect("A exists");
        assert_eq!(through_a.value, None, "{key:?} through A");
        assert_eq!(
            through_a.receipt,
            miss_receipt(GET_ABSENT),
            "a miss through A costs the fixed miss price"
        );
    }

    // The same prefix scan: thousands of entries in B, none in A at cost 5.
    let in_b = head.scan_prefix(world.b, b"b/", traced).expect("B exists");
    assert_eq!(in_b.value.len(), B_TOTAL_RECORDS);
    assert_eq!(in_b.receipt.spent, Cost::new(B_SCAN_ALL_COST));
    assert_eq!(in_b.receipt.outcome, Outcome::Applied);
    let through_a = head.scan_prefix(world.a, b"b/", traced).expect("A exists");
    assert!(through_a.value.is_empty());
    assert_eq!(through_a.receipt, miss_receipt(SCAN_FOREIGN_PREFIX));

    // The other direction and the empty namespace behave the same way.
    let a_through_b = head.scan_prefix(world.b, b"a/", traced).expect("B exists");
    assert!(a_through_b.value.is_empty());
    assert_eq!(a_through_b.receipt, miss_receipt(SCAN_FOREIGN_PREFIX));
    let a_key_through_b = head.get(world.b, b"a/07", traced).expect("B exists");
    assert_eq!(a_key_through_b.value, None);
    assert_eq!(a_key_through_b.receipt, miss_receipt(GET_ABSENT));
    let everything_in_c = head.scan_prefix(world.c, b"", traced).expect("C exists");
    assert!(everything_in_c.value.is_empty());
    assert_eq!(everything_in_c.receipt, miss_receipt(SCAN_FOREIGN_PREFIX));
    assert_eq!(
        everything_in_c.physical.record_nodes, 0,
        "C has no record node"
    );
    let a_key_through_c = head.get(world.c, b"a/07", traced).expect("C exists");
    assert_eq!(a_key_through_c.value, None);
    assert_eq!(a_key_through_c.receipt, miss_receipt(GET_ABSENT));
}
