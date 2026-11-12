//! Block lifecycle and publication: ordered candidates over retained
//! snapshots, atomic publication, and stale and foreign candidates.
//!
//! Every expected cost below is a literal computed by hand from the FIXTURE
//! weights (`support/pricing.rs`) and the model-version-3 charge points:
//!
//! | operation  | units                                      |
//! | ---------- | ------------------------------------------ |
//! | create     | 1 + 50 + RootUpdate 1*2 = 53               |
//! | transfer   | 1 + 20 + RootUpdate 1*2 = 23               |
//! | put        | 1 + 10 + 2 * key_len + value_len + 2*2     |
//! | delete     | 1 + 5 + 2 * key_len + 2*2                  |
//! | get        | 1 + 3 + value_len (present), 4 (missing)   |
//! | scan       | 1 + 4 + sum over entries (2 + key + value) |
//! | descriptor | 1                                          |
//!
//! A rejection after the directory lookup (unknown namespace, unauthorized,
//! out of budget on the first group) has spent exactly 1.

use arkiv_db::{
    Budget, Cost, Effect, Engine, EngineError, Entry, NamespaceDescriptor, NamespaceId,
    OperationClass, Outcome, OwnerId, PriceSchedule, ReadResult, ReceiptKind, Rejection, Revision,
    SnapshotId, WriteOp,
};

use crate::support::{
    reader::Reader,
    vocab::{
        ALICE, BOB, CAROL, CanonicalRead, DAVE, add_namespace, applied, apply, begin, canonical,
        canonical_read, create_namespace, delete, descriptor, entry, ns, publish, put,
        read_applied, read_rejected, receipt, rejected, transfer,
    },
};

const ERIN: OwnerId = OwnerId::new([0xe0; 20]);

/// Unlimited, untraced budget: the cost is asserted through the receipt.
const ALL: Budget = Budget::unlimited();

fn value_of<T>(result: ReadResult<T>) -> T {
    result
        .unwrap_or_else(|rejected| panic!("read rejected: {rejected}"))
        .value
}

fn some(bytes: &[u8]) -> Option<Vec<u8>> {
    Some(bytes.to_vec())
}

/// Everything a reader reports about one namespace, receipts included
/// (traced, so ledgers take part in the comparison).
#[derive(Clone, Debug, PartialEq, Eq)]
struct Observation {
    namespaces: usize,
    descriptor: CanonicalRead<NamespaceDescriptor>,
    gets: Vec<CanonicalRead<Option<Vec<u8>>>>,
    scan: CanonicalRead<Vec<Entry>>,
}

fn observe(reader: &impl Reader, namespace: NamespaceId, keys: &[&[u8]]) -> Observation {
    let traced = Budget::unlimited().traced();
    Observation {
        namespaces: reader.namespace_count(),
        descriptor: canonical_read(reader.descriptor(namespace, traced)),
        gets: keys
            .iter()
            .map(|key| canonical_read(reader.get(namespace, key, traced)))
            .collect(),
        scan: canonical_read(reader.scan_prefix(namespace, b"", traced)),
    }
}

#[test]
fn three_blocks_publish_ordered_writes_a_transfer_and_rejections() {
    let mut engine = Engine::new(PriceSchedule::FIXTURE);
    assert_eq!(engine.head(), SnapshotId::GENESIS);

    // Block 1: one namespace per owner. create = 1 + 50 + 2 = 53 each.
    let mut candidate = begin(&engine, Some(1_000));
    let shop = apply(&mut candidate, ALICE, create_namespace("test"));
    let blog = apply(&mut candidate, BOB, create_namespace("test"));
    assert_eq!(
        (shop.effect, shop.receipt),
        (Effect::NamespaceCreated(ns(1)), applied(53))
    );
    assert_eq!(
        (blog.effect, blog.receipt),
        (Effect::NamespaceCreated(ns(2)), applied(53))
    );
    let (shop, blog) = (ns(1), ns(2));
    let sealed = candidate.seal();
    assert_eq!(sealed.spent(), Cost::new(106), "53 + 53");
    assert_eq!(sealed.receipts(), &[applied(53), applied(53)]);
    let snapshot_1 = engine.publish(&sealed).unwrap();
    assert_eq!(snapshot_1.sequence(), 1);
    let after_block_1 = engine.head_snapshot();
    assert_eq!(after_block_1.id(), snapshot_1);
    assert_eq!(after_block_1.namespace_count(), 2);
    assert_eq!(
        value_of(after_block_1.descriptor(shop, ALL)),
        descriptor(shop, ALICE, 0)
    );
    assert_eq!(
        value_of(after_block_1.descriptor(blog, ALL)),
        descriptor(blog, BOB, 0)
    );
    assert_eq!(after_block_1.record_count(shop), Some(0));

    // Block 2: ordered operations, including put/delete/put on one
    // key inside the block. Only the last write survives; head never shows
    // an intermediate value while the candidate is open.
    let mut candidate = begin(&engine, Some(1_000));
    /// Actor, operation, expected effect, expected cost, expected value of
    /// `k` in the branch afterwards.
    type Step<'a> = (OwnerId, WriteOp, Effect, u64, Option<&'a [u8]>);
    let steps: [Step<'_>; 6] = [
        // put k=v1: 1 + 10 + 2*1 + 2 + 4 = 19
        (
            ALICE,
            put(shop, b"k", b"v1"),
            Effect::Put { replaced: false },
            19,
            Some(b"v1"),
        ),
        // delete k: 1 + 5 + 2*1 + 4 = 12
        (
            ALICE,
            delete(shop, b"k"),
            Effect::Deleted { existed: true },
            12,
            None,
        ),
        // put k=final: 1 + 10 + 2*1 + 5 + 4 = 22
        (
            ALICE,
            put(shop, b"k", b"final"),
            Effect::Put { replaced: false },
            22,
            Some(b"final"),
        ),
        // put other=x: 1 + 10 + 2*5 + 1 + 4 = 26
        (
            ALICE,
            put(shop, b"other", b"x"),
            Effect::Put { replaced: false },
            26,
            Some(b"final"),
        ),
        // put other=y (replacement, same size): 26 again
        (
            ALICE,
            put(shop, b"other", b"y"),
            Effect::Put { replaced: true },
            26,
            Some(b"final"),
        ),
        // bob's put post=hello: 1 + 10 + 2*4 + 5 + 4 = 28
        (
            BOB,
            put(blog, b"post", b"hello"),
            Effect::Put { replaced: false },
            28,
            Some(b"final"),
        ),
    ];
    for (index, (actor, op, effect, cost, k_in_branch)) in steps.into_iter().enumerate() {
        let executed = apply(&mut candidate, actor, op);
        assert_eq!(executed.effect, effect, "step {index}");
        assert_eq!(executed.receipt, applied(cost), "step {index}");
        assert_eq!(
            value_of(candidate.get(shop, b"k", ALL)),
            k_in_branch.map(<[u8]>::to_vec),
            "branch view of shop/k after step {index}"
        );
        assert_eq!(
            value_of(after_block_1.get(shop, b"k", ALL)),
            None,
            "retained block-1 handle after step {index}"
        );
        assert_eq!(
            value_of(engine.head_snapshot().get(shop, b"k", ALL)),
            None,
            "head after step {index}"
        );
        assert_eq!(engine.head(), snapshot_1, "head id after step {index}");
    }
    let sealed = candidate.seal();
    assert_eq!(
        sealed.receipts(),
        &[
            applied(19),
            applied(12),
            applied(22),
            applied(26),
            applied(26),
            applied(28)
        ]
    );
    assert_eq!(
        sealed.spent(),
        Cost::new(133),
        "19 + 12 + 22 + 26 + 26 + 28"
    );
    let snapshot_2 = engine.publish(&sealed).unwrap();
    assert_eq!(snapshot_2.sequence(), 2);
    let after_block_2 = engine.head_snapshot();
    assert_eq!(after_block_2.id(), snapshot_2);
    assert_eq!(after_block_2.base(), Some(snapshot_1));
    assert_eq!(value_of(after_block_2.get(shop, b"k", ALL)), some(b"final"));
    assert_eq!(value_of(after_block_2.get(shop, b"other", ALL)), some(b"y"));
    assert_eq!(
        value_of(after_block_2.scan_prefix(shop, b"", ALL)),
        vec![entry(b"k", b"final"), entry(b"other", b"y")],
        "the published snapshot shows only the last write per key"
    );
    assert_eq!(after_block_2.record_count(shop), Some(2));
    assert_eq!(
        value_of(after_block_2.get(blog, b"post", ALL)),
        some(b"hello")
    );
    assert_eq!(after_block_2.record_count(blog), Some(1));
    let shop_keys: &[&[u8]] = &[b"k", b"other", b"k3"];

    // Block 3: transfer the shop to bob without moving any record,
    // then reject an unauthorized put, an underfunded put, and a put whose
    // budget exceeds the block's remainder. Block budget 32.
    let mut candidate = begin(&engine, Some(32));
    let transferred = apply(&mut candidate, ALICE, transfer(shop, 0, BOB));
    assert_eq!(
        transferred.effect,
        Effect::Transferred {
            revision: Revision(1)
        }
    );
    assert_eq!(transferred.receipt, applied(23), "1 + 20 + 2");
    assert_eq!(
        transferred.physical.record_nodes, 0,
        "a transfer visits no record node"
    );
    assert_eq!(candidate.remaining(), Cost::new(9), "32 - 23");
    assert_eq!(
        value_of(candidate.descriptor(shop, ALL)),
        descriptor(shop, BOB, 1)
    );
    let shop_after_transfer = observe(&candidate, shop, shop_keys);

    // Alice no longer owns the shop: rejected after the directory lookup.
    let (reason, receipt) =
        canonical(&candidate.execute(ALICE, put(shop, b"k3", b"unauthorized"), ALL)).unwrap_err();
    assert_eq!(reason, Rejection::Unauthorized);
    assert_eq!(receipt, rejected(1, Rejection::Unauthorized));
    assert_eq!(candidate.remaining(), Cost::new(8), "32 - 23 - 1");

    // Bob's put costs 1 + 10 + 2*2 + 4 + 4 = 23; with a 5-unit budget the
    // lookup is charged and the write group (22) is refused as a whole.
    let (reason, receipt) =
        canonical(&candidate.execute(BOB, put(shop, b"k3", b"over"), Budget::limit(5)))
            .unwrap_err();
    assert_eq!(reason, Rejection::OutOfBudget);
    assert_eq!(receipt, rejected(1, Rejection::OutOfBudget));
    assert_eq!(candidate.remaining(), Cost::new(7), "32 - 23 - 1 - 1");

    // A budget above the block's remainder is refused at admission: no charge
    // and no receipt.
    let (reason, receipt) =
        canonical(&candidate.execute(BOB, put(shop, b"k3", b"x"), Budget::limit(8))).unwrap_err();
    assert_eq!(
        reason,
        Rejection::ExceedsBlockBudget {
            remaining: Cost::new(7)
        }
    );
    assert_eq!(
        receipt,
        rejected(
            0,
            Rejection::ExceedsBlockBudget {
                remaining: Cost::new(7)
            }
        )
    );
    assert_eq!(candidate.remaining(), Cost::new(7));
    assert_eq!(candidate.spent(), Cost::new(25), "23 + 1 + 1");
    assert_eq!(candidate.receipts().len(), 3);
    assert_eq!(
        observe(&candidate, shop, shop_keys),
        shop_after_transfer,
        "rejected operations change nothing"
    );

    let sealed = candidate.seal();
    assert_eq!(
        sealed.receipts(),
        &[
            applied(23),
            rejected(1, Rejection::Unauthorized),
            rejected(1, Rejection::OutOfBudget)
        ]
    );
    assert_eq!(sealed.spent(), Cost::new(25));
    let snapshot_3 = engine.publish(&sealed).unwrap();
    assert_eq!(snapshot_3.sequence(), 3);
    let after_block_3 = engine.head_snapshot();
    assert_eq!(
        value_of(after_block_3.descriptor(shop, ALL)),
        descriptor(shop, BOB, 1)
    );
    assert_eq!(after_block_3.record_count(shop), Some(2));
    assert!(
        after_block_2.records_shared_with(&after_block_3, shop),
        "the transfer left the shop's record map physically untouched"
    );
    assert!(after_block_2.records_shared_with(&after_block_3, blog));
    assert_eq!(value_of(after_block_3.get(shop, b"k3", ALL)), None);

    assert_eq!(engine.snapshot_count(), 4);
}

#[test]
fn empty_blocks_publish_new_snapshots_with_identical_answers() {
    let mut engine = Engine::new(PriceSchedule::FIXTURE);
    let mut setup = begin(&engine, None);
    let store = add_namespace(&mut setup, ALICE, "test");
    apply(&mut setup, ALICE, put(store, b"a", b"1"));
    apply(&mut setup, ALICE, put(store, b"bb", b"22"));
    let filled = publish(&mut engine, setup);
    let before = engine.head_snapshot();
    let keys: &[&[u8]] = &[b"a", b"bb", b"zz"];
    let baseline = observe(&before, store, keys);
    // Literal expectations for the baseline, so the equalities below are not
    // vacuous.
    assert_eq!(
        canonical_read(before.get(store, b"a", ALL)),
        Ok((some(b"1"), read_applied(5))),
        "1 + 3 + 1"
    );
    assert_eq!(
        canonical_read(before.get(store, b"bb", ALL)),
        Ok((some(b"22"), read_applied(6))),
        "1 + 3 + 2"
    );
    assert_eq!(
        canonical_read(before.get(store, b"zz", ALL)),
        Ok((None, read_applied(4))),
        "1 + 3, nothing to copy"
    );
    assert_eq!(
        canonical_read(before.scan_prefix(store, b"", ALL)),
        Ok((
            vec![entry(b"a", b"1"), entry(b"bb", b"22")],
            read_applied(15)
        )),
        "1 + 4 + (2 + 1 + 1) + (2 + 2 + 2)"
    );
    assert_eq!(
        canonical_read(before.descriptor(store, ALL)),
        Ok((descriptor(store, ALICE, 0), read_applied(1)))
    );

    // A candidate with no operations, under a zero block budget, still
    // publishes a fresh snapshot.
    let empty = begin(&engine, Some(0)).seal();
    assert!(empty.receipts().is_empty());
    assert_eq!(empty.spent(), Cost::ZERO);
    assert_eq!(empty.base(), filled);
    let first_empty = engine.publish(&empty).unwrap();
    assert_eq!(first_empty.sequence(), filled.sequence() + 1);
    assert_eq!(engine.head(), first_empty);
    let after = engine.snapshot(first_empty).unwrap();
    assert_eq!(after.id(), first_empty);
    assert_eq!(after.base(), Some(filled));
    assert_eq!(observe(&after, store, keys), baseline);
    assert_eq!(after.namespace_count(), before.namespace_count());
    assert_eq!(after.record_count(store), before.record_count(store));
    assert_eq!(after.record_count(store), Some(2));
    assert!(after.records_shared_with(&before, store));
    assert_eq!(after.pricing(), before.pricing());

    // Several consecutive empty blocks.
    let mut previous = first_empty;
    for round in 1..=5 {
        let empty = begin(&engine, None);
        let id = publish(&mut engine, empty);
        assert_eq!(id.sequence(), previous.sequence() + 1, "round {round}");
        let snapshot = engine.snapshot(id).unwrap();
        assert_eq!(snapshot.base(), Some(previous), "round {round}");
        assert_eq!(observe(&snapshot, store, keys), baseline, "round {round}");
        assert_eq!(snapshot.record_count(store), Some(2), "round {round}");
        assert!(
            snapshot.records_shared_with(&before, store),
            "round {round}"
        );
        previous = id;
    }
    assert_eq!(engine.head(), previous);
    assert_eq!(
        engine.snapshot_count(),
        8,
        "genesis, one filled and six empty"
    );
    assert_eq!(
        observe(&before, store, keys),
        baseline,
        "the old handle is unchanged"
    );
}

struct Quartet {
    a: NamespaceId,
    b: NamespaceId,
    c: NamespaceId,
    d: NamespaceId,
}

/// The four facts a mixed block changes, as one reader reports them.
#[derive(Clone, Debug, PartialEq, Eq)]
struct MixedView {
    a_k: Option<Vec<u8>>,
    b_k: Option<Vec<u8>>,
    c: CanonicalRead<NamespaceDescriptor>,
    d: CanonicalRead<NamespaceDescriptor>,
    namespaces: usize,
}

fn mixed_view(reader: &impl Reader, q: &Quartet) -> MixedView {
    MixedView {
        a_k: value_of(reader.get(q.a, b"k", ALL)),
        b_k: value_of(reader.get(q.b, b"k", ALL)),
        c: canonical_read(reader.descriptor(q.c, ALL)),
        d: canonical_read(reader.descriptor(q.d, ALL)),
        namespaces: reader.namespace_count(),
    }
}

fn before_mixed_block(q: &Quartet) -> MixedView {
    MixedView {
        a_k: some(b"a0"),
        b_k: some(b"b0"),
        c: Ok((descriptor(q.c, CAROL, 0), read_applied(1))),
        d: Err((
            Rejection::UnknownNamespace,
            read_rejected(1, Rejection::UnknownNamespace),
        )),
        namespaces: 3,
    }
}

fn after_mixed_block(q: &Quartet) -> MixedView {
    MixedView {
        a_k: some(b"a1"),
        b_k: None,
        c: Ok((descriptor(q.c, DAVE, 1), read_applied(1))),
        d: Ok((descriptor(q.d, ERIN, 0), read_applied(1))),
        namespaces: 4,
    }
}

#[test]
fn a_block_touching_four_namespaces_becomes_visible_all_at_once() {
    let mut engine = Engine::new(PriceSchedule::FIXTURE);
    let mut setup = begin(&engine, None);
    let q = Quartet {
        a: add_namespace(&mut setup, ALICE, "test"),
        b: add_namespace(&mut setup, BOB, "test"),
        c: add_namespace(&mut setup, CAROL, "test"),
        d: ns(4),
    };
    assert_eq!((q.a, q.b, q.c), (ns(1), ns(2), ns(3)));
    apply(&mut setup, ALICE, put(q.a, b"k", b"a0"));
    apply(&mut setup, BOB, put(q.b, b"k", b"b0"));
    let base = publish(&mut engine, setup);
    let h = engine.head_snapshot();
    assert_eq!(h.id(), base);
    let before = before_mixed_block(&q);
    let after = after_mixed_block(&q);
    assert_eq!(mixed_view(&h, &q), before);

    let mut candidate = begin(&engine, Some(1_000));
    let steps: [(OwnerId, WriteOp, Effect, u64); 4] = [
        // put A/k=a1: 1 + 10 + 2*1 + 2 + 4 = 19
        (
            ALICE,
            put(q.a, b"k", b"a1"),
            Effect::Put { replaced: true },
            19,
        ),
        // delete B/k: 1 + 5 + 2*1 + 4 = 12
        (
            BOB,
            delete(q.b, b"k"),
            Effect::Deleted { existed: true },
            12,
        ),
        // transfer C: 1 + 20 + 2 = 23
        (
            CAROL,
            transfer(q.c, 0, DAVE),
            Effect::Transferred {
                revision: Revision(1),
            },
            23,
        ),
        // create D: 1 + 50 + 2 = 53
        (
            ERIN,
            create_namespace("test"),
            Effect::NamespaceCreated(q.d),
            53,
        ),
    ];
    for (index, (actor, op, effect, cost)) in steps.into_iter().enumerate() {
        let executed = apply(&mut candidate, actor, op);
        assert_eq!(executed.effect, effect, "step {index}");
        assert_eq!(executed.receipt, applied(cost), "step {index}");
        assert_eq!(
            mixed_view(&h, &q),
            before,
            "retained head handle after step {index}"
        );
        assert_eq!(
            mixed_view(&engine.head_snapshot(), &q),
            before,
            "fresh head handle after step {index}"
        );
        assert_eq!(engine.head(), base, "head id after step {index}");
        let expected_branch = MixedView {
            a_k: after.a_k.clone(),
            b_k: if index >= 1 {
                after.b_k.clone()
            } else {
                before.b_k.clone()
            },
            c: if index >= 2 {
                after.c.clone()
            } else {
                before.c.clone()
            },
            d: if index >= 3 {
                after.d.clone()
            } else {
                before.d.clone()
            },
            namespaces: if index >= 3 { 4 } else { 3 },
        };
        assert_eq!(
            mixed_view(&candidate, &q),
            expected_branch,
            "branch view after step {index}"
        );
    }
    assert_eq!(mixed_view(&candidate, &q), after);

    let sealed = candidate.seal();
    assert_eq!(
        sealed.receipts(),
        &[applied(19), applied(12), applied(23), applied(53)]
    );
    assert_eq!(sealed.spent(), Cost::new(107), "19 + 12 + 23 + 53");
    assert_eq!(mixed_view(&h, &q), before, "retained handle after seal");
    assert_eq!(
        mixed_view(&engine.head_snapshot(), &q),
        before,
        "fresh head after seal"
    );
    assert_eq!(engine.head(), base);

    let published_id = engine.publish(&sealed).unwrap();
    assert_eq!(published_id.sequence(), base.sequence() + 1);
    let published = engine.snapshot(published_id).unwrap();
    assert_eq!(published.base(), Some(base));
    assert_eq!(
        mixed_view(&published, &q),
        after,
        "all four changes together"
    );
    assert_eq!(mixed_view(&engine.head_snapshot(), &q), after);
    assert_eq!(
        mixed_view(&h, &q),
        before,
        "the old handle shows none of them"
    );
    assert_eq!(
        mixed_view(&engine.snapshot(base).unwrap(), &q),
        before,
        "the old snapshot shows none of them"
    );
    assert_eq!(
        (h.record_count(q.b), published.record_count(q.b)),
        (Some(1), Some(0))
    );
    assert_eq!(
        (h.record_count(q.d), published.record_count(q.d)),
        (None, Some(0))
    );
    assert!(!published.records_shared_with(&h, q.a));
    assert!(!published.records_shared_with(&h, q.b));
}

#[test]
fn a_stale_candidate_loses_and_releases_its_namespace_ordinal() {
    let mut engine = Engine::new(PriceSchedule::FIXTURE);
    let mut setup = begin(&engine, None);
    let a = add_namespace(&mut setup, ALICE, "test");
    let c = add_namespace(&mut setup, CAROL, "test");
    apply(&mut setup, ALICE, put(a, b"k", b"a0"));
    let h = publish(&mut engine, setup);

    let c1 = begin(&engine, None);
    let mut c2 = begin(&engine, None);
    assert_eq!((c1.base(), c2.base()), (h, h));
    // put A/k=a1: 1 + 10 + 2*1 + 2 + 4 = 19; transfer C: 23; create: 53.
    apply(&mut c2, ALICE, put(a, b"k", b"a1"));
    apply(&mut c2, CAROL, transfer(c, 0, DAVE));
    let n = add_namespace(&mut c2, ERIN, "test");
    assert_eq!(n, ns(3));
    let branch_receipts = c2.receipts().to_vec();
    let branch_spent = c2.spent();
    assert_eq!(branch_receipts, vec![applied(19), applied(23), applied(53)]);
    assert_eq!(branch_spent, Cost::new(95), "19 + 23 + 53");
    assert_eq!(c2.namespace_count(), 3);
    let loser = c2.seal();
    let winner = c1.seal();

    let h1 = engine.publish(&winner).unwrap();
    assert_eq!(h1.sequence(), h.sequence() + 1);
    assert_eq!(
        engine.publish(&loser),
        Err(EngineError::StaleHead {
            expected: h,
            head: h1
        })
    );
    assert_eq!(engine.head(), h1);
    assert_eq!(engine.snapshot_count(), 3);
    let head = engine.head_snapshot();
    assert_eq!(value_of(head.get(a, b"k", ALL)), some(b"a0"));
    assert_eq!(value_of(head.descriptor(c, ALL)), descriptor(c, CAROL, 0));
    assert_eq!(head.namespace_count(), 2);
    assert_eq!(
        canonical_read(head.descriptor(n, ALL)),
        Err((
            Rejection::UnknownNamespace,
            read_rejected(1, Rejection::UnknownNamespace)
        ))
    );

    // The rejected candidate keeps what it charged.
    assert_eq!(loser.base(), h);
    assert_eq!(loser.receipts(), branch_receipts.as_slice());
    assert_eq!(loser.spent(), branch_spent);

    // The loser never held ordinal 3: a fresh candidate on the new head
    // allocates it.
    let mut fresh = begin(&engine, None);
    assert_eq!(fresh.base(), h1);
    assert_eq!(add_namespace(&mut fresh, ERIN, "test"), n);
    assert_eq!(
        engine.begin(h, None).map(|_| ()),
        Err(EngineError::StaleHead {
            expected: h,
            head: h1
        })
    );
    let fresh = fresh.seal();
    let h2 = engine.publish(&fresh).unwrap();
    assert_eq!(h2.sequence(), h.sequence() + 2);
    assert_eq!(
        engine.publish(&fresh),
        Err(EngineError::StaleHead {
            expected: h1,
            head: h2
        }),
        "the same sealed candidate cannot publish twice"
    );
    assert_eq!(engine.head(), h2);
    assert_eq!(engine.snapshot_count(), 4);
    assert_eq!(engine.head_snapshot().namespace_count(), 3);
    assert_eq!(
        value_of(engine.head_snapshot().descriptor(n, ALL)),
        descriptor(n, ERIN, 0)
    );
    assert_eq!(
        engine.publish(&loser),
        Err(EngineError::StaleHead {
            expected: h,
            head: h2
        }),
        "the loser reports the current head each time"
    );
}

#[test]
fn a_block_published_in_two_stages_keeps_its_intermediate_snapshot_readable() {
    let mut engine = Engine::new(PriceSchedule::FIXTURE);

    // Block 1, stage 1: an admin candidate commits schedule 2, which reprices
    // PointRead from 3 to 6 and keeps every other FIXTURE weight.
    let repriced = PriceSchedule::FIXTURE
        .with_id(2)
        .with_price(OperationClass::PointRead, 6);
    let mut admin = begin(&engine, None);
    assert_eq!(admin.set_pricing(repriced.clone()), Ok(()));
    assert_eq!(
        admin.pricing().id(),
        1,
        "the admin branch stays pinned to its base schedule"
    );
    let admin_snapshot = publish(&mut engine, admin);
    assert_eq!(admin_snapshot.sequence(), 1);

    // Block 1, stage 2: a data candidate priced with schedule 2.
    let mut data = begin(&engine, None);
    assert_eq!(data.pricing(), &repriced);
    let store = add_namespace(&mut data, ALICE, "test");
    assert_eq!(store, ns(1));
    let written = apply(&mut data, ALICE, put(store, b"k", b"v"));
    assert_eq!(
        written.receipt,
        receipt(ReceiptKind::Write, 2, 18, Outcome::Applied),
        "1 + 10 + 2*1 + 1 + 4 under schedule 2"
    );
    let sealed = data.seal();
    assert_eq!(
        sealed.receipts(),
        &[
            receipt(ReceiptKind::Write, 2, 53, Outcome::Applied),
            receipt(ReceiptKind::Write, 2, 18, Outcome::Applied)
        ]
    );
    let data_snapshot = engine.publish(&sealed).unwrap();
    assert_eq!(data_snapshot.sequence(), 2);

    // The intermediate snapshot is not a block, but it is still readable.
    let intermediate = engine.snapshot(admin_snapshot).unwrap();
    assert_eq!(intermediate.id(), admin_snapshot);
    assert_eq!(intermediate.base(), Some(SnapshotId::GENESIS));
    assert_eq!(intermediate.namespace_count(), 0);
    assert_eq!(intermediate.pricing(), &repriced);
    assert_eq!(
        canonical_read(intermediate.descriptor(store, ALL)),
        Err((
            Rejection::UnknownNamespace,
            receipt(
                ReceiptKind::Read,
                2,
                1,
                Outcome::Rejected(Rejection::UnknownNamespace)
            )
        ))
    );
    assert_eq!(
        engine.snapshot(SnapshotId::GENESIS).unwrap().pricing(),
        &PriceSchedule::FIXTURE
    );
    let terminal = engine.snapshot(data_snapshot).unwrap();
    assert_eq!(terminal.base(), Some(admin_snapshot));
    assert_eq!(
        canonical_read(terminal.get(store, b"k", ALL)),
        Ok((
            some(b"v"),
            receipt(ReceiptKind::Read, 2, 8, Outcome::Applied)
        )),
        "1 + 6 + 1 under schedule 2"
    );

    // Block 2 is empty and still gets a snapshot of its own.
    let empty = begin(&engine, None);
    let empty_snapshot = publish(&mut engine, empty);
    assert_eq!(empty_snapshot.sequence(), 3);
    assert_eq!(engine.snapshot_count(), 4);
    assert_eq!(
        value_of(
            engine
                .snapshot(empty_snapshot)
                .unwrap()
                .get(store, b"k", ALL)
        ),
        some(b"v")
    );
}

#[test]
fn candidates_sealed_by_another_engine_are_foreign_even_at_genesis() {
    let mut x = Engine::new(PriceSchedule::FIXTURE);
    let mut y = Engine::new(PriceSchedule::FIXTURE);
    assert_eq!(
        (x.head(), y.head()),
        (SnapshotId::GENESIS, SnapshotId::GENESIS)
    );

    let empty = x.begin(SnapshotId::GENESIS, None).unwrap().seal();
    let mut creating = x.begin(SnapshotId::GENESIS, None).unwrap();
    assert_eq!(add_namespace(&mut creating, ALICE, "test"), ns(1));
    let creating = creating.seal();

    assert_eq!(y.publish(&empty), Err(EngineError::ForeignCandidate));
    assert_eq!(y.publish(&creating), Err(EngineError::ForeignCandidate));
    assert_eq!(y.head(), SnapshotId::GENESIS);
    assert_eq!(y.snapshot_count(), 1);
    assert_eq!(y.head_snapshot().namespace_count(), 0);

    // X accepts its own candidate; it stays foreign to Y, and the instance
    // check comes before the stale-head check.
    assert_eq!(x.publish(&creating).map(SnapshotId::sequence), Ok(1));
    assert_eq!(y.publish(&creating), Err(EngineError::ForeignCandidate));
    let x_head = x.head();
    assert_eq!(
        x.publish(&empty),
        Err(EngineError::StaleHead {
            expected: SnapshotId::GENESIS,
            head: x_head
        })
    );

    // The relation is symmetric, even with both engines at the same sequence.
    let own = y.begin(SnapshotId::GENESIS, None).unwrap().seal();
    assert_eq!(y.publish(&own).map(SnapshotId::sequence), Ok(1));
    assert_eq!(x.head(), y.head());
    assert_eq!(x.publish(&own), Err(EngineError::ForeignCandidate));
    assert_eq!(x.snapshot_count(), 2);
    assert_eq!(x.head_snapshot().namespace_count(), 1);
    assert_eq!(y.head_snapshot().namespace_count(), 0);
}

#[test]
fn every_one_of_fifty_published_blocks_stays_readable_and_unchanged() {
    let mut engine = Engine::new(PriceSchedule::FIXTURE);
    let store = ns(1);
    let mut retained = vec![engine.head_snapshot()];
    // Per block: "k" is deleted every fifth block and otherwise set to v{n};
    // "seq" always becomes the block number.
    let mut expected_k: Vec<Option<Vec<u8>>> = vec![None];
    for block in 1..=50u64 {
        let mut candidate = begin(&engine, None);
        if block == 1 {
            assert_eq!(add_namespace(&mut candidate, ALICE, "test"), store);
        }
        let k = if block % 5 == 0 {
            apply(&mut candidate, ALICE, delete(store, b"k"));
            None
        } else {
            let value = format!("v{block}").into_bytes();
            apply(&mut candidate, ALICE, put(store, b"k", &value));
            Some(value)
        };
        apply(
            &mut candidate,
            ALICE,
            put(store, b"seq", block.to_string().as_bytes()),
        );
        let id = publish(&mut engine, candidate);
        assert_eq!(id.sequence(), block);
        retained.push(engine.head_snapshot());
        expected_k.push(k);
    }
    assert_eq!(engine.snapshot_count(), 51);

    for block in 0..=50u64 {
        let index = usize::try_from(block).unwrap();
        let by_id = engine.snapshot(retained[index].id()).unwrap();
        for (label, snapshot) in [("retained", &retained[index]), ("engine.snapshot", &by_id)] {
            assert_eq!(snapshot.id().sequence(), block, "{label} {block}");
            if block == 0 {
                assert_eq!(snapshot.base(), None, "{label} genesis");
                assert_eq!(snapshot.namespace_count(), 0, "{label} genesis");
                assert_eq!(
                    canonical_read(snapshot.get(store, b"k", ALL)),
                    Err((
                        Rejection::UnknownNamespace,
                        read_rejected(1, Rejection::UnknownNamespace)
                    )),
                    "{label} genesis"
                );
                continue;
            }
            assert_eq!(
                snapshot.base().map(SnapshotId::sequence),
                Some(block - 1),
                "{label} {block}"
            );
            assert_eq!(snapshot.namespace_count(), 1, "{label} {block}");
            assert_eq!(
                value_of(snapshot.get(store, b"k", ALL)),
                expected_k[index],
                "{label} {block}: k"
            );
            assert_eq!(
                value_of(snapshot.get(store, b"seq", ALL)),
                Some(block.to_string().into_bytes()),
                "{label} {block}: seq"
            );
            assert_eq!(
                snapshot.record_count(store),
                Some(if expected_k[index].is_some() { 2 } else { 1 }),
                "{label} {block}: record count"
            );
        }
    }
}

#[test]
fn branch_reads_see_uncommitted_writes_that_head_never_shows() {
    let mut engine = Engine::new(PriceSchedule::FIXTURE);
    let mut setup = begin(&engine, None);
    let store = add_namespace(&mut setup, ALICE, "test");
    apply(&mut setup, ALICE, put(store, b"k", b"old"));
    apply(&mut setup, ALICE, put(store, b"gone", b"bye"));
    let base = publish(&mut engine, setup);
    let keys: &[&[u8]] = &[b"k", b"gone", b"fresh"];
    let committed = observe(&engine.head_snapshot(), store, keys);
    let committed_other = observe(&engine.head_snapshot(), ns(2), keys);

    let mut candidate = begin(&engine, Some(1_000));
    // put k=new: 1 + 10 + 2*1 + 3 + 4 = 20
    let replaced = apply(&mut candidate, ALICE, put(store, b"k", b"new"));
    assert_eq!(
        (replaced.effect, replaced.receipt),
        (Effect::Put { replaced: true }, applied(20))
    );
    // delete gone: 1 + 5 + 2*4 + 4 = 18
    let removed = apply(&mut candidate, ALICE, delete(store, b"gone"));
    assert_eq!(
        (removed.effect, removed.receipt),
        (Effect::Deleted { existed: true }, applied(18))
    );
    // put fresh=!: 1 + 10 + 2*5 + 1 + 4 = 26
    let inserted = apply(&mut candidate, ALICE, put(store, b"fresh", b"!"));
    assert_eq!(
        (inserted.effect, inserted.receipt),
        (Effect::Put { replaced: false }, applied(26))
    );
    // create: 53
    assert_eq!(add_namespace(&mut candidate, BOB, "test"), ns(2));
    assert_eq!(candidate.spent(), Cost::new(117), "20 + 18 + 26 + 53");

    // The branch answers from its working state.
    assert_eq!(
        canonical_read(candidate.get(store, b"k", ALL)),
        Ok((some(b"new"), read_applied(7))),
        "1 + 3 + 3"
    );
    assert_eq!(
        canonical_read(candidate.get(store, b"gone", ALL)),
        Ok((None, read_applied(4)))
    );
    assert_eq!(
        canonical_read(candidate.get(store, b"fresh", ALL)),
        Ok((some(b"!"), read_applied(5))),
        "1 + 3 + 1"
    );
    assert_eq!(
        canonical_read(candidate.scan_prefix(store, b"", ALL)),
        Ok((
            vec![entry(b"fresh", b"!"), entry(b"k", b"new")],
            read_applied(19)
        )),
        "1 + 4 + (2 + 5 + 1) + (2 + 1 + 3)"
    );
    assert_eq!(
        canonical_read(candidate.descriptor(ns(2), ALL)),
        Ok((descriptor(ns(2), BOB, 0), read_applied(1)))
    );
    assert_eq!(candidate.namespace_count(), 2);

    // The head answers from the published state.
    let head = engine.head_snapshot();
    assert_eq!(head.id(), base);
    assert_eq!(
        canonical_read(head.get(store, b"k", ALL)),
        Ok((some(b"old"), read_applied(7)))
    );
    assert_eq!(
        canonical_read(head.get(store, b"gone", ALL)),
        Ok((some(b"bye"), read_applied(7)))
    );
    assert_eq!(
        canonical_read(head.get(store, b"fresh", ALL)),
        Ok((None, read_applied(4)))
    );
    assert_eq!(
        canonical_read(head.scan_prefix(store, b"", ALL)),
        Ok((
            vec![entry(b"gone", b"bye"), entry(b"k", b"old")],
            read_applied(20)
        )),
        "1 + 4 + (2 + 4 + 3) + (2 + 1 + 3)"
    );
    assert_eq!(
        canonical_read(head.descriptor(ns(2), ALL)),
        Err((
            Rejection::UnknownNamespace,
            read_rejected(1, Rejection::UnknownNamespace)
        ))
    );
    assert_eq!(head.namespace_count(), 1);
    assert_eq!(observe(&head, store, keys), committed);

    // Reads are not block work.
    assert_eq!(candidate.spent(), Cost::new(117));
    assert_eq!(candidate.remaining(), Cost::new(883));
    assert_eq!(candidate.receipts().len(), 4);

    // Dropping the unsealed branch changes nothing.
    drop(candidate);
    assert_eq!(engine.head(), base);
    assert_eq!(engine.snapshot_count(), 2);
    let head = engine.head_snapshot();
    assert_eq!(observe(&head, store, keys), committed);
    assert_eq!(observe(&head, ns(2), keys), committed_other);
    assert_eq!(head.record_count(store), Some(2));
    assert_eq!(head.record_count(ns(2)), None);

    // The discarded creation released its ordinal.
    let mut again = begin(&engine, None);
    assert_eq!(add_namespace(&mut again, CAROL, "test"), ns(2));
    drop(again);
    assert_eq!(engine.head_snapshot().namespace_count(), 1);
    assert_eq!(engine.head(), base);
}
