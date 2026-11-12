//! Historical reads during commits, shared by `history.rs` and the
//! experiment example.

use std::{
    sync::{
        RwLock,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

use arkiv_db_redb::{RedbStore, testing::SplitMix};
use arkiv_db_storage::versioned::{CommitBatch, SpaceId, VersionedStore};

use super::{Chain, assert_applied, commit};

/// What one run committed and read back.
#[derive(Clone, Debug)]
pub struct ConcurrentHistory {
    /// Every committed block, in order.
    pub blocks: Vec<CommitBatch>,
    /// Objects the readers verified while the commits ran.
    pub verified_objects: u64,
    /// Wall-clock time of the commits after the first.
    pub elapsed: Duration,
}

/// Commits `blocks` blocks (block `height` holds `objects_at(height)`) while
/// four readers keep verifying every object and record of random published
/// blocks.
pub fn concurrent_history(
    store: &RedbStore,
    blocks: u64,
    objects_at: impl Fn(u64) -> Vec<(SpaceId, u64, usize)>,
) -> ConcurrentHistory {
    let published: RwLock<Vec<CommitBatch>> = RwLock::default();
    let done = AtomicBool::new(false);
    let verified = AtomicU64::new(0);

    let mut chain = Chain::default();
    let first = chain.next(&objects_at(0));
    commit(store, &first);
    published.write().unwrap().push(first);

    let started = Instant::now();
    std::thread::scope(|scope| {
        for reader in 0..4 {
            let (published, done, verified) = (&published, &done, &verified);
            scope.spawn(move || {
                let mut rng = SplitMix::new(reader);
                while !done.load(Ordering::Acquire) {
                    // Blocks are published after `Committed`, so every block
                    // listed before reading the head is at or below it.
                    let batch = {
                        let list = published.read().unwrap();
                        let head = store.head().unwrap().expect("genesis committed");
                        assert!(list.len() as u64 <= head.height + 1);
                        list[rng.below(list.len() as u64) as usize].clone()
                    };
                    assert_applied(store, &batch);
                    verified.fetch_add(batch.objects.len() as u64, Ordering::Relaxed);
                }
            });
        }
        for height in 1..blocks {
            let batch = chain.next(&objects_at(height));
            commit(store, &batch);
            published.write().unwrap().push(batch);
        }
        done.store(true, Ordering::Release);
    });
    ConcurrentHistory {
        blocks: published.into_inner().unwrap(),
        verified_objects: verified.into_inner(),
        elapsed: started.elapsed(),
    }
}
