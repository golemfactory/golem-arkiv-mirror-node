//! Nodes rehashed per block against the `RootUpdate` bound.
//!
//! Model 3 prepays root maintenance with one `RootUpdate` per logical map
//! entry write. The cost that prepayment covers is the trie nodes a block
//! builds and must hash: an entry write in a map with `L`-byte keys creates
//! at most `2L + 2` nodes (`docs/state-trie.md`): catalog, rows and
//! postings (8 bytes) 18, raw records (41) 84, host (32) 66, key bindings
//! (33) 68, terms (164) 330.
//!
//! Measurement: every block runs on an engine reopened from the previous
//! block's committed objects (`Engine::from_state` over an in-memory object
//! store), so the new nodes of the sealed candidate are exactly the nodes
//! the block built that its state still reaches. Two counts per map:
//! - hashed nodes (`SealedCandidate::hashed_nodes`): the new nodes that
//!   carry a hash (every new map root and every new node of 32 bytes or
//!   more; smaller nodes are inline, encoded inside their parent), counted
//!   wherever the state reaches them. Computing the state root hashes each
//!   of them once; on top of them a block computes one state-header digest.
//! - staged objects (`SealedCandidate::staged_objects`): the same nodes,
//!   each `(space, map, id)` once, which is what a commit writes. Identical
//!   nodes built by different writes of a block (the one-entry posting maps
//!   of a new record's terms, say) are hashed once each but staged once, so
//!   staged objects never exceed hashed nodes.
//!
//! What this file establishes:
//! - For every block of a seeded mixed workload (raw writes, typed
//!   creates, patches and deletes, transfers, namespace creation, host
//!   writes), each map's staged objects are at most its hashed nodes, and
//!   those at most the number of that map's entry writes times its bound,
//!   where the entry writes are read off each operation's `RootUpdate`
//!   charge (checked against the ledger), plus the uncharged host writes.
//! - Adversarial deep paths, with counts derived by hand from the crafted
//!   key sets (derivations at each fixture): 66 crafted record keys force a
//!   branch at every nibble a 33-byte key encoding can branch on; creating
//!   one more key there rehashes exactly 66 key-map nodes, deleting the
//!   deepest crafted key 66 again, and deleting the added key 65. 218 terms
//!   force a 175-node path
//!   through the term map, the longest a valid term allows (only the name
//!   length and the high nibbles of the tag and presence bytes can never
//!   differ while the bytes before them agree; the value length can, since
//!   a `Str` value may end in zero bytes); adding a record to that term
//!   rehashes exactly 176 term nodes.
//!
//! The measured numbers are printed with `--nocapture` and reported in
//! `docs/state-trie.md`. What this file does not establish: time, memory or
//! storage I/O of hashing or committing.

use std::{collections::BTreeMap, sync::Arc};

use crate::trie_support::Rng;
use arkiv_db::{
    Budget, Cell, CellName, Engine, Limits, MapKind, OperationClass, OwnerId, PriceSchedule,
    Receipt, SealedCandidate, Value, WriteOp,
};

use crate::support::{
    object_store::{ObjectStore, reopen, stage},
    vocab::{
        ALICE, BOB, attr, begin, charged, create_namespace, create_record, delete, delete_record,
        field, name, ns, patch_record, put, transfer,
    },
};

/// Nodes one entry write can create in each map: `2L + 2`.
fn bound(map: MapKind) -> u64 {
    let key_len: u64 = match map {
        MapKind::Catalog | MapKind::Rows | MapKind::Postings => 8,
        // Escaped raw keys: at most two bytes per key byte plus a terminator.
        MapKind::Records => 41,
        MapKind::Host => 32,
        MapKind::Keys => 33,
        // A name and its terminator, tag, presence, a default-limit `Str`
        // and its terminator: 33 + 2 + 129.
        MapKind::Terms => (CellName::MAX_LEN + 3 + Limits::DEFAULT_INDEXED_LEN + 1) as u64,
    };
    2 * key_len + 2
}

/// One step of a block.
#[derive(Clone, Debug)]
enum BlockStep {
    Write(OwnerId, WriteOp),
    HostPut([u8; 32], Vec<u8>),
    HostRemove([u8; 32]),
}

/// What a block hashed and staged and what its entry writes allow, per
/// map.
#[derive(Debug, Default)]
struct Measurement {
    hashed: BTreeMap<MapKind, u64>,
    staged: BTreeMap<MapKind, u64>,
    writes: BTreeMap<MapKind, u64>,
}

impl Measurement {
    fn hashed(&self, map: MapKind) -> u64 {
        self.hashed.get(&map).copied().unwrap_or(0)
    }

    fn staged(&self, map: MapKind) -> u64 {
        self.staged.get(&map).copied().unwrap_or(0)
    }

    fn allowed(&self, map: MapKind) -> u64 {
        self.writes.get(&map).copied().unwrap_or(0) * bound(map)
    }

    fn total_hashed(&self) -> u64 {
        self.hashed.values().sum()
    }

    fn total_staged(&self) -> u64 {
        self.staged.values().sum()
    }

    fn total_allowed(&self) -> u64 {
        self.writes
            .iter()
            .map(|(map, count)| count * bound(*map))
            .sum()
    }
}

struct Chain {
    store: Arc<ObjectStore>,
    engine: Engine,
}

impl Chain {
    fn new() -> Self {
        let genesis = Engine::new(PriceSchedule::FIXTURE).head_snapshot();
        let (store, engine) = reopen(&genesis.state_header(), Vec::new());
        Self { store, engine }
    }

    /// Runs `steps` as one block, measures it, commits its objects and
    /// reopens the engine on the new state.
    fn block(&mut self, steps: &[BlockStep]) -> Measurement {
        let mut branch = begin(&self.engine, None);
        let mut measurement = Measurement::default();
        for step in steps {
            match step {
                BlockStep::Write(actor, op) => {
                    let executed = branch
                        .execute(*actor, op.clone(), Budget::unlimited().traced())
                        .unwrap_or_else(|rejected| panic!("{op:?}: {rejected}"));
                    count_writes(op, &executed.receipt, &mut measurement);
                }
                BlockStep::HostPut(key, value) => {
                    branch.try_host_put(*key, value).unwrap().unwrap();
                    *measurement.writes.entry(MapKind::Host).or_default() += 1;
                }
                BlockStep::HostRemove(key) => {
                    branch.try_host_remove(key).unwrap();
                    *measurement.writes.entry(MapKind::Host).or_default() += 1;
                }
            }
        }
        let sealed = branch.seal();
        measurement.hashed = sealed.hashed_nodes();
        for object in sealed.staged_objects() {
            *measurement.staged.entry(object.map).or_default() += 1;
        }
        self.commit(&sealed);
        measurement
    }

    fn commit(&mut self, sealed: &SealedCandidate) {
        stage(&self.store, sealed.staged_objects());
        let header = sealed.state_header();
        self.engine = Engine::from_state(&header, Limits::DEFAULT, self.store.clone()).unwrap();
        assert_eq!(
            self.engine.head_snapshot().state_root(),
            sealed.state_root()
        );
    }
}

/// The entry writes an applied operation made, per map, from the charge
/// table: every write rewrites its namespace's catalog leaf; a raw write one
/// raw entry; a record creation or deletion a key binding and a row; a
/// patch a row; every index insert or removal a posting entry and a term
/// leaf. Their number must be the operation's `RootUpdate` count.
fn count_writes(op: &WriteOp, receipt: &Receipt, measurement: &mut Measurement) {
    let ledger = receipt.ledger.as_ref().expect("traced");
    let index =
        charged(ledger, OperationClass::IndexInsert) + charged(ledger, OperationClass::IndexRemove);
    let writes: Vec<(MapKind, u64)> = match op {
        WriteOp::CreateNamespace { .. } | WriteOp::TransferNamespace { .. } => {
            vec![(MapKind::Catalog, 1)]
        }
        WriteOp::Put { .. } | WriteOp::Delete { .. } => {
            vec![(MapKind::Catalog, 1), (MapKind::Records, 1)]
        }
        WriteOp::CreateRecord { .. } | WriteOp::DeleteRecord { .. } => vec![
            (MapKind::Catalog, 1),
            (MapKind::Keys, 1),
            (MapKind::Rows, 1),
            (MapKind::Postings, index),
            (MapKind::Terms, index),
        ],
        WriteOp::PatchRecord { .. } => vec![
            (MapKind::Catalog, 1),
            (MapKind::Rows, 1),
            (MapKind::Postings, index),
            (MapKind::Terms, index),
        ],
    };
    let entry_writes: u64 = writes.iter().map(|(_, count)| count).sum();
    assert_eq!(
        entry_writes,
        charged(ledger, OperationClass::RootUpdate),
        "{op:?}: RootUpdate counts the entry writes"
    );
    for (map, count) in writes {
        *measurement.writes.entry(map).or_default() += count;
    }
}

fn assert_within_bounds(measurement: &Measurement, context: &str) {
    for map in [
        MapKind::Catalog,
        MapKind::Host,
        MapKind::Records,
        MapKind::Keys,
        MapKind::Rows,
        MapKind::Terms,
        MapKind::Postings,
    ] {
        assert!(
            measurement.staged(map) <= measurement.hashed(map),
            "{context}: {map:?} staged {} > hashed {}",
            measurement.staged(map),
            measurement.hashed(map)
        );
        assert!(
            measurement.hashed(map) <= measurement.allowed(map),
            "{context}: {map:?} hashed {} > {}",
            measurement.hashed(map),
            measurement.allowed(map)
        );
    }
    assert!(measurement.total_hashed() <= measurement.total_allowed());
}

fn report(context: &str, measurement: &Measurement) {
    let per_map: Vec<String> = measurement
        .writes
        .keys()
        .chain(measurement.hashed.keys())
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .map(|map| {
            format!(
                "{map:?} {}/{}/{}",
                measurement.staged(*map),
                measurement.hashed(*map),
                measurement.allowed(*map)
            )
        })
        .collect();
    println!(
        "{context}: staged {}, hashed {} of at most {} (+1 header digest); \
         per map staged/hashed/allowed: {}",
        measurement.total_staged(),
        measurement.total_hashed(),
        measurement.total_allowed(),
        per_map.join(", ")
    );
}

fn write(op: WriteOp) -> BlockStep {
    BlockStep::Write(ALICE, op)
}

fn create(namespace: u64, key: &[u8], cells: Vec<(&str, Cell)>) -> BlockStep {
    let cells = cells
        .into_iter()
        .map(|(text, cell)| (name(text), cell))
        .collect();
    write(create_record(ns(namespace), key, cells))
}

#[test]
fn every_block_of_a_mixed_workload_stays_within_the_root_update_bound() {
    let mut chain = Chain::new();
    let mut rng = Rng(0x5eed_4e11);
    let base: Vec<BlockStep> = ["alpha", "beta"]
        .into_iter()
        .map(|text| write(create_namespace(text)))
        .chain((0..200u64).map(|index| {
            create(
                1 + index % 2,
                format!("r{index:03}").as_bytes(),
                vec![
                    (
                        "kind",
                        attr(Value::str(["red", "rose"][(index % 2) as usize])),
                    ),
                    ("n", attr(Value::I64(index as i64 % 17))),
                    ("$exp", attr(Value::U64(index % 5))),
                    ("note", field(Value::bytes(b"n"))),
                ],
            )
        }))
        .collect();
    let measurement = chain.block(&base);
    assert_within_bounds(&measurement, "base block");
    report("base block (2 namespaces, 200 records)", &measurement);

    let mut live: Vec<(u64, Vec<u8>)> = (0..200u64)
        .map(|index| (1 + index % 2, format!("r{index:03}").into_bytes()))
        .collect();
    let mut next = 200u64;
    let mut revisions = [0u64; 3];
    let mut owners = [ALICE; 3];
    for height in 2..=21 {
        let mut steps = Vec::new();
        for _ in 0..1 + rng.below(30) {
            let namespace = 1 + rng.below(2) as u64;
            let owner = owners[namespace as usize];
            let step = match rng.below(9) {
                0 | 1 => {
                    next += 1;
                    let key = format!("r{next:03}").into_bytes();
                    live.push((namespace, key.clone()));
                    let cells = vec![(name("kind"), attr(Value::str("ruby")))];
                    BlockStep::Write(owner, create_record(ns(namespace), &key, cells))
                }
                2 | 3 if !live.is_empty() => {
                    let (namespace, key) = live[rng.below(live.len())].clone();
                    let set = vec![(name("n"), attr(Value::I64(rng.below(40) as i64)))];
                    BlockStep::Write(
                        owners[namespace as usize],
                        patch_record(ns(namespace), &key, set, vec![name("$exp")]),
                    )
                }
                4 if !live.is_empty() => {
                    let (namespace, key) = live.swap_remove(rng.below(live.len()));
                    BlockStep::Write(
                        owners[namespace as usize],
                        delete_record(ns(namespace), &key),
                    )
                }
                5 => {
                    let key = format!("raw{}", rng.below(50));
                    let value = vec![7; rng.below(8)];
                    BlockStep::Write(owner, put(ns(namespace), key.as_bytes(), &value))
                }
                6 => {
                    let key = format!("raw{}", rng.below(50));
                    BlockStep::Write(owner, delete(ns(namespace), key.as_bytes()))
                }
                7 => {
                    let to = if owner == ALICE { BOB } else { ALICE };
                    let revision = revisions[namespace as usize];
                    let step = BlockStep::Write(owner, transfer(ns(namespace), revision, to));
                    revisions[namespace as usize] += 1;
                    owners[namespace as usize] = to;
                    step
                }
                _ => {
                    let mut key = [0u8; 32];
                    key[0] = b'D';
                    key[8] = rng.below(8) as u8;
                    if rng.below(3) == 0 {
                        BlockStep::HostRemove(key)
                    } else {
                        BlockStep::HostPut(key, 1u64.to_be_bytes().to_vec())
                    }
                }
            };
            steps.push(step);
        }
        let measurement = chain.block(&steps);
        assert_within_bounds(&measurement, &format!("block {height}"));
        report(
            &format!("block {height} ({} steps)", steps.len()),
            &measurement,
        );
    }
    // An empty block hashes and stages nothing.
    let measurement = chain.block(&[]);
    assert_eq!(measurement.total_hashed(), 0);
    assert_eq!(measurement.total_staged(), 0);
}

/// 66 record keys that force a branch at every nibble a key encoding can
/// branch on (a 32-byte key encodes as `len ‖ bytes`, 33 bytes, 66
/// nibbles):
///
/// - the spine: 32 zero bytes, encoded `20 00 … 00`;
/// - `00` (one byte), encoded `01 00`: first nibble 0 against the spine's
///   2;
/// - for each nibble j of the 32 key bytes (encoding nibbles 2 to 65), 32
///   bytes that are zero except that nibble, which is 1.
///
/// Nibble 1 (the length's low nibble, 0 for 32) cannot differ: a length of
/// 0x21 to 0x2f is above 32. The key trie is therefore a branch on nibble 0
/// (the `00` leaf and the 32-byte keys), an extension over nibble 1, and a
/// branch on each of nibbles 2 to 65 with the crafted key's leaf beside the
/// spine: 1 + 1 + 64 + the spine's leaf = 67 nodes on the spine's path,
/// `2·33 + 1`, the most any 33-byte key path holds.
fn crafted_keys() -> Vec<Vec<u8>> {
    let mut keys = vec![vec![0u8; 32], vec![0u8]];
    for nibble in 0..64 {
        let mut key = vec![0u8; 32];
        key[nibble / 2] = if nibble % 2 == 0 { 0x10 } else { 0x01 };
        keys.push(key);
    }
    keys
}

#[test]
fn a_deep_key_path_rehashes_its_whole_path() {
    let mut chain = Chain::new();
    let mut base = vec![write(create_namespace("keys"))];
    base.extend(crafted_keys().iter().map(|key| create(1, key, Vec::new())));
    assert_eq!(base.len(), 67);
    let measurement = chain.block(&base);
    assert_within_bounds(&measurement, "base");

    // One more key on the spine's path: 31 zero bytes and 02. It follows
    // the spine to the branch on nibble 65, whose slot 2 is empty, and adds
    // a leaf there. The block rebuilds the root branch, the extension and
    // the 64 branches on nibbles 2 to 65 (66 nodes), and creates the leaf:
    // 67 nodes. The leaf (an empty path and an 8-byte id: 11 bytes) is
    // inline, and every rebuilt node is at least 32 bytes (each holds a
    // 33-byte reference, or three 11-byte inline leaves in the deepest
    // branch), so exactly 66 key-map nodes are hashed, all distinct, and
    // staged.
    let mut deeper = vec![0u8; 31];
    deeper.push(0x02);
    let measurement = chain.block(&[create(1, &deeper, Vec::new())]);
    assert_within_bounds(&measurement, "insert on the deep path");
    assert_eq!(measurement.hashed(MapKind::Keys), 66);
    assert_eq!(measurement.staged(MapKind::Keys), 66);
    assert_eq!(measurement.allowed(MapKind::Keys), 68);
    report("one key inserted on the 67-node key path", &measurement);

    // Removing the crafted key that parts at nibble 65 (last byte 01): the
    // branch on nibble 65 keeps the spine's leaf and 02's leaf, so nothing
    // collapses; the path is rebuilt (root, extension, branches 2 to 65: 66
    // nodes).
    let mut shallower = vec![0u8; 31];
    shallower.push(0x01);
    let measurement = chain.block(&[write(delete_record(ns(1), &shallower))]);
    assert_within_bounds(&measurement, "removal on the deep path");
    assert_eq!(measurement.hashed(MapKind::Keys), 66);
    assert_eq!(measurement.staged(MapKind::Keys), 66);

    // Removing 02 again leaves the branch on nibble 65 with the spine's leaf
    // alone: it collapses into that leaf (path [0], 11 bytes, inline), so
    // the rebuilt path is the root, the extension and the branches on
    // nibbles 2 to 64: 65 objects, within the removal bound 2·33.
    let measurement = chain.block(&[write(delete_record(ns(1), &deeper))]);
    assert_within_bounds(&measurement, "collapse on the deep path");
    assert_eq!(measurement.hashed(MapKind::Keys), 65);
    assert_eq!(measurement.staged(MapKind::Keys), 65);
    report(
        "one key removed, collapsing the deepest branch",
        &measurement,
    );
}

/// A 20-character name of `a`s with `replacement` at `position`.
fn name_with(position: usize, replacement: char) -> String {
    let mut text: Vec<char> = vec!['a'; 20];
    text[position] = replacement;
    text.into_iter().collect()
}

/// A 64-character value of `a`s with `replacement` at `position`.
fn value_with(position: usize, replacement: char) -> String {
    let mut text: Vec<char> = vec!['a'; 64];
    text[position] = replacement;
    text.into_iter().collect()
}

#[test]
fn a_deep_term_path_rehashes_its_whole_path() {
    // The spine term S = (name `a`×20, Str, value `a`×62 ‖ NUL: 63 bytes).
    // Its 87-byte key (state layout 2): name bytes 0x61 (nibbles 0–39: 6 1
    // 6 1 …), the name terminator 00 (40, 41), tag Str = 02 (42, 43),
    // presence 01 (44, 45), the value shifted by one: 0x62 ×62 (46–169),
    // NUL as 0x01 (170, 171), and the value terminator 00 (172, 173).
    //
    // Terms that part from S at a nibble force a branch there:
    // - nibbles 0–39: 40 other names, `q` (0x71) for a high nibble or `b`
    //   (0x62) for a low one at each position of `a`×20;
    // - nibble 40: the name `a`×21 (0x61 against the terminator 00);
    // - nibble 42: the same name with type I64 (0x11 against 0x02);
    // - nibble 43: the same name with type Bool (0x01 against 0x02);
    // - nibble 45: S's existence term (presence 00);
    // - nibbles 46–169: 124 other values, `q` or `b` at each of the
    //   positions 0–61 of `a`×64 (shifted 0x72 or 0x63 against 0x62);
    // - nibbles 170, 171: `a`×62 ‖ `q` (0x72) and `a`×62 ‖ U+0001 (0x02)
    //   against 0x01; `a`×62 itself (its terminator 00) also branches at 171;
    // - nibbles 172, 173: `a`×62 ‖ NUL ‖ `q` (0x72) and `a`×62 ‖ NUL ‖ NUL
    //   or ‖ U+0001 (0x01, 0x02) against S's terminator 00.
    // Nibble 41 (the low nibble of the name terminator: names never contain
    // 0x01–0x0f) and nibble 44 (presence high nibble) cannot differ while
    // every byte before them agrees. So S's path is 172 branches (nibbles
    // 0–40, 42, 43 and 45–173), an extension over nibble 41, one over
    // nibble 44, and S's leaf (empty path): 175 nodes, against the `2·87 +
    // 1` = 175 of an unconstrained 87-byte key, minus the two fixed nibbles
    // plus their two extensions.
    let spine_name = "a".repeat(20);
    let prefix = "a".repeat(62);
    let spine_value = format!("{prefix}\0");
    let mut chain = Chain::new();
    let mut base = vec![write(create_namespace("terms"))];
    base.push(create(
        1,
        b"spine",
        vec![(spine_name.as_str(), attr(Value::str(&spine_value)))],
    ));
    let names: Vec<String> = (0..20)
        .flat_map(|position| [name_with(position, 'q'), name_with(position, 'b')])
        .collect();
    base.push(create(
        1,
        b"names",
        names
            .iter()
            .map(|text| (text.as_str(), attr(Value::str("v"))))
            .collect(),
    ));
    base.push(create(
        1,
        b"tag",
        vec![(spine_name.as_str(), attr(Value::I64(1)))],
    ));
    base.push(create(
        1,
        b"bool",
        vec![(spine_name.as_str(), attr(Value::Bool(true)))],
    ));
    let longer_name = "a".repeat(21);
    base.push(create(
        1,
        b"longer",
        vec![(longer_name.as_str(), attr(Value::str("v")))],
    ));
    let mut values: Vec<String> = (0..62)
        .flat_map(|position| [value_with(position, 'q'), value_with(position, 'b')])
        .collect();
    values.extend([
        format!("{prefix}q"),
        format!("{prefix}\u{1}"),
        format!("{prefix}\0q"),
        format!("{prefix}\0\u{1}"),
        format!("{prefix}\0\0"),
        prefix,
    ]);
    assert_eq!(values.len(), 130);
    for (index, value) in values.iter().enumerate() {
        base.push(create(
            1,
            format!("v{index:03}").as_bytes(),
            vec![(spine_name.as_str(), attr(Value::str(value)))],
        ));
    }
    let measurement = chain.block(&base);
    assert_within_bounds(&measurement, "base");
    // S, its existence term, 130 values, the I64 and Bool value and
    // existence terms, and the 41 other names' value and existence terms.
    let terms = chain
        .engine
        .head_snapshot()
        .term_count(ns(1))
        .expect("the namespace exists");
    assert_eq!(terms, 1 + 1 + 130 + 2 + 2 + 2 * 41);

    // One more record holding S: S's posting and S's existence posting
    // gain its id, so both term leaves are rewritten. Rewriting S rebuilds
    // its whole 175-node path; the existence leaf hangs directly off the
    // presence branch (nibble 45) and adds one node: 176. Every one of them
    // is at least 32 bytes (each branch and extension holds a 33-byte
    // reference to the rest of the path, and both leaves hold 40-byte
    // posting handles), and no two are equal (distinct leaves, and each
    // path node references the next), so all 176 are hashed and staged.
    let measurement = chain.block(&[create(
        1,
        b"another",
        vec![(spine_name.as_str(), attr(Value::str(&spine_value)))],
    )]);
    assert_within_bounds(&measurement, "insert on the deep term path");
    assert_eq!(measurement.writes[&MapKind::Terms], 2);
    assert_eq!(measurement.hashed(MapKind::Terms), 176);
    assert_eq!(measurement.staged(MapKind::Terms), 176);
    assert_eq!(measurement.allowed(MapKind::Terms), 2 * 330);
    report("a record added to the 175-node term path", &measurement);
}
