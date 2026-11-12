//! An I/O error at every mutating call of a commit, shared by
//! `reconciliation.rs` and the experiment example.

use std::io;

use arkiv_db_redb::testing::CrashDisk;
use arkiv_db_storage::versioned::{
    CommitOutcome, Health, Reconciliation, StorageErrorKind, VersionedStore, reconcile,
};

use super::{
    assert_absent, assert_applied, commit, commit_trace, config, crash_store, durable_image,
    fail_call, lost_sync,
};

/// Outcomes of one sweep, by commit outcome and what reconciliation found.
#[derive(Clone, Copy, Debug, Default)]
pub struct IoErrorSweep {
    /// Mutating backend calls of a complete commit: one failure each.
    pub calls: u64,
    pub not_committed: u64,
    pub indeterminate_absent: u64,
    pub indeterminate_committed: u64,
}

/// Fails each mutating call of the candidate's commit in turn (not a crash:
/// the device keeps what it was given) and checks that the store latches,
/// that reconciliation after `recover` finds the parent or the candidate,
/// and that the candidate ends up committed exactly once.
pub fn io_error_sweep() -> IoErrorSweep {
    let (genesis, candidate) = lost_sync::chain();
    let image = durable_image(config(true), std::slice::from_ref(&genesis));
    let trace = commit_trace(&image, config(true), &candidate);

    let mut sweep = IoErrorSweep {
        calls: trace.len() as u64,
        ..IoErrorSweep::default()
    };
    for (k, op) in trace.iter().enumerate() {
        let disk = CrashDisk::from_image(image.clone());
        let store = crash_store(&disk, config(true));
        fail_call(&disk, &trace, k, io::ErrorKind::Other);
        let outcome = store.commit(&candidate);
        assert!(
            matches!(store.health(), Health::Failed(_)),
            "call {k} {op:?}: {outcome:?}"
        );
        // Every call fails until recovery, even though the device is fine.
        assert!(store.head().is_err());
        match reconcile(&store, &candidate).unwrap() {
            Reconciliation::Committed => {
                assert!(
                    matches!(outcome, CommitOutcome::Indeterminate(_)),
                    "call {k} {op:?}: {outcome:?} but committed"
                );
                sweep.indeterminate_committed += 1;
                assert_applied(&store, &candidate);
            }
            Reconciliation::NotCommitted => {
                match outcome {
                    CommitOutcome::NotCommitted(_) => sweep.not_committed += 1,
                    CommitOutcome::Indeterminate(_) => sweep.indeterminate_absent += 1,
                    CommitOutcome::Committed => panic!("call {k}: Committed but absent"),
                }
                assert_absent(&store, &candidate);
                commit(&store, &candidate);
                assert_applied(&store, &candidate);
            }
            Reconciliation::Diverged { head } => panic!("call {k}: diverged at {head:?}"),
        }
        assert!(matches!(
            store.commit(&candidate),
            CommitOutcome::NotCommitted(error) if error.kind == StorageErrorKind::HeadMismatch
        ));
        assert_applied(&store, &genesis);
    }
    sweep
}
