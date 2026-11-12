//! What the durable-engine tests share: a genesis, the one-record workload,
//! commit-fault groups and the fixtures of the bulk and simulator profiles.

use std::sync::Arc;

use arkiv_db::{
    B256, Budget, NamespaceDescriptor, PriceSchedule, Revision, SealedCandidate, Snapshot,
    SnapshotId, StorageErrorKind,
    durable::{
        CommitInfo, DurableEngine, EngineGenesis, Initialization, Options, Publication,
        simulator::{SimulatorIdentity, SimulatorPacket},
    },
};
use arkiv_db_storage::{testing::CommitFault, versioned::VersionedStore};

use super::vocab::{OWNER, add_namespace, apply, namespace_name, ns, put};

/// Commit faults after which the store holds nothing new.
pub const FAULTS_NOT_APPLIED: [CommitFault; 2] = [
    CommitFault::NotCommitted(StorageErrorKind::Full),
    CommitFault::IndeterminateNotApplied(StorageErrorKind::Io),
];
/// Commit faults after which the store holds the batch, although the caller
/// was told no success.
pub const FAULTS_APPLIED: [CommitFault; 2] = [
    CommitFault::IndeterminateApplied(StorageErrorKind::Io),
    CommitFault::AckLost,
];

pub fn genesis() -> EngineGenesis {
    EngineGenesis::new([0x42; 32], PriceSchedule::FIXTURE)
}

pub fn snapshot_id(sequence: u64) -> SnapshotId {
    SnapshotId::from_sequence(sequence)
}

pub fn committed(initialization: Initialization) -> DurableEngine {
    match initialization {
        Initialization::Committed(engine) => engine,
        other => panic!("initialization did not commit: {other:?}"),
    }
}

/// A new engine on `store` with [`genesis`].
pub fn initialize(store: Arc<dyn VersionedStore>, options: Options) -> DurableEngine {
    committed(DurableEngine::initialize(store, genesis(), options).unwrap())
}

/// Prepares and commits `candidate`; the store must report it committed.
pub fn commit(engine: &mut DurableEngine, candidate: SealedCandidate) -> CommitInfo {
    let info = engine.prepare(candidate).unwrap();
    assert_eq!(engine.commit().unwrap(), Publication::Committed(info));
    info
}

/// A candidate on the head that sets `"key"` in namespace 1 to `value`,
/// creating the namespace first on an empty engine.
pub fn put_candidate(engine: &DurableEngine, value: &[u8]) -> SealedCandidate {
    let mut branch = engine.begin(engine.head(), None).unwrap();
    if engine.head() == snapshot_id(0) {
        add_namespace(&mut branch, OWNER, "alpha");
    }
    apply(&mut branch, OWNER, put(ns(1), b"key", value));
    branch.try_seal().unwrap()
}

/// What [`put_candidate`] stored, as `snapshot` sees it.
pub fn value(snapshot: &Snapshot) -> Vec<u8> {
    snapshot
        .try_get(ns(1), b"key", Budget::unlimited())
        .unwrap()
        .unwrap()
        .value
        .unwrap()
}

/// No head, no record and no object was ever written to `store`.
pub fn assert_untouched(store: &dyn VersionedStore) {
    assert_eq!(store.head().unwrap(), None);
    let stats = store.stats();
    assert_eq!(stats.records_written, 0);
    assert!(
        stats
            .spaces
            .values()
            .all(|space| space.objects_written == 0)
    );
}

/// `keccak(len(domain) ‖ domain ‖ bytes)`, built by hand for format vectors.
pub fn domain_hash(domain: &str, bytes: &[u8]) -> B256 {
    let mut preimage = vec![u8::try_from(domain.len()).unwrap()];
    preimage.extend(domain.as_bytes());
    preimage.extend(bytes);
    alloy_primitives::keccak256(preimage)
}

/// The namespace a bulk-built genesis holds.
pub fn entities() -> NamespaceDescriptor {
    NamespaceDescriptor {
        id: ns(1),
        name: namespace_name("entities"),
        owner: arkiv_db::OwnerId::new([1; 20]),
        revision: Revision::INITIAL,
    }
}

pub fn simulator_identity() -> SimulatorIdentity {
    SimulatorIdentity {
        source_id: [1; 16],
        run_id: [2; 16],
        node_genesis_hash: B256::repeat_byte(3),
        metadata: b"canonical host genesis fixture".to_vec(),
    }
}

/// The node identity, genesis and height-zero packet a host would supply
/// to turn the `staged` state into a simulator database.
pub fn node(staged: &EngineGenesis) -> (SimulatorIdentity, EngineGenesis, SimulatorPacket) {
    let identity = simulator_identity();
    let genesis = EngineGenesis {
        deployment: identity.deployment(),
        max_object_bytes: 8 * 1024 * 1024,
        ..staged.clone()
    };
    let packet = SimulatorPacket {
        height: 0,
        state_root: staged.initial_header().root(),
        bytes: b"genesis block record".to_vec(),
        inclusions: vec![],
    };
    (identity, genesis, packet)
}
