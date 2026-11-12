//! Gate 1 (atomicity): a crash at any point of a multi-table commit leaves
//! either the parent state or the candidate state, never a mix.
//!
//! Setup: a `RedbStore` on a simulated `CrashDisk` holds committed blocks.
//! The candidate block writes objects to the catalog, host and two existing
//! namespace tables, creates a new namespace table, and inserts records into
//! every record table (metadata, blocks, manifests, block index, inclusions)
//! plus the head: one redb transaction over ten tables. The candidate is
//! committed from two starting points:
//!
//! - **after a restart**: three parent blocks, crashed and reopened (redb's
//!   recovery runs) just before the candidate commit;
//! - **in steady state**: twelve parent blocks committed through the same
//!   store just before the candidate, so earlier commits have freed pages
//!   for reuse, redb's write buffer holds pages from its post-commit step,
//!   and the allocator state is live rather than freshly loaded.
//!
//! For every crash point k — the candidate commit is cut off at its k-th
//! mutating backend call (write, sync or length change), for every k from 0
//! up to and including the number of calls a complete commit makes — the
//! test builds crash images from the frozen device: only synced data, all
//! unsynced operations, and seeded subsets of the unsynced operations with
//! each write torn at 512-byte-sector or byte granularity. Each image is
//! reopened as a new store (running redb's recovery) and must hold exactly
//! the parent (every candidate object and record absent) or exactly the
//! candidate (every candidate object and record present), with all earlier
//! blocks intact, and must pass redb's own integrity check (page checksums
//! and an allocator state matching the reachable pages). `reconcile` must
//! agree, and a reconciled-absent candidate must then commit exactly once.
//! One more block then commits on top: every earlier block must still read
//! back, before and after a power cut following that commit, and the new
//! image must pass the integrity check again — so an allocator state saved
//! wrongly at the crash, which could make a later commit overwrite live
//! pages, would be detected.
//!
//! A commit that returned `Committed` must survive a crash with only synced
//! data (immediate durability, also covered by `durability.rs`).
//!
//! Quick repair on (the store's setting) must never need redb's full repair
//! after any of these crashes. With quick repair off, the same crash points
//! must also be atomic; the full-repair count is reported.
//!
//! Scope: the device model is the one documented in
//! `arkiv_db_redb::testing` (completed syncs durable, unsynced writes
//! persisting in any subset, torn at sector or byte granularity). It does not
//! model firmware that lies about syncs, reordering across a completed sync,
//! or bit rot.
//!
//! The `#[ignore]`d heavy runs:
//! `cargo test -p arkiv-db-redb --release --test it atomicity:: -- --ignored`.

use crate::common::{
    commit_trace, config,
    crash::{
        HEAVY_SEEDS, SEEDS, Tally, candidate, check_image, parent_image, run, run_growing,
        run_growing_steady, run_steady, sampled,
    },
    crash_store,
};
use arkiv_db_redb::testing::{CrashDisk, CrashMode};
use arkiv_db_storage::versioned::VersionedStore;

#[test]
fn every_crash_point_is_parent_or_candidate_with_quick_repair() {
    let tally = run(true, SEEDS);
    println!("ten-table commit, quick repair: {tally:?}");
    assert!(tally.parent > 0 && tally.candidate > 0);
    assert_eq!(tally.full_repairs, 0, "quick repair must avoid full repair");
}

#[test]
#[ignore = "heavy: 64 torn subsets per granularity and crash point"]
fn every_crash_point_is_parent_or_candidate_heavy() {
    let tally = run(true, HEAVY_SEEDS);
    assert_eq!(tally.full_repairs, 0);
    let tally = run(false, HEAVY_SEEDS);
    assert!(tally.parent > 0 && tally.candidate > 0);
}

#[test]
fn every_crash_point_is_parent_or_candidate_without_quick_repair() {
    let tally = run(false, SEEDS);
    println!("ten-table commit, no quick repair: {tally:?}");
    assert!(tally.parent > 0 && tally.candidate > 0);
}

#[test]
fn every_crash_point_in_steady_state_is_parent_or_candidate() {
    let tally = run_steady(true, SEEDS);
    println!("ten-table commit in steady state, quick repair: {tally:?}");
    assert!(tally.parent > 0 && tally.candidate > 0);
    assert_eq!(tally.full_repairs, 0, "quick repair must avoid full repair");
}

/// Sampled crash points of the spilling, growing commit in steady state.
#[test]
fn growing_commit_in_steady_state_is_parent_or_candidate() {
    let tally = run_growing_steady(Some(&sampled(61)), 1);
    println!("growing, spilling commit in steady state: {tally:?}");
}

#[test]
fn head_is_parent_until_the_commit_call_returns_into_its_final_phase() {
    // The head a crash leaves depends only on how far the commit got: once a
    // crash image holds the candidate for some crash point, every later
    // crash point's "all unsynced" image holds it too (monotone publication).
    let (image, parents) = parent_image(config(true));
    let candidate = candidate(parents.last().unwrap().new_head);
    let calls = commit_trace(&image, config(true), &candidate).len() as u64;

    let mut seen_candidate = false;
    let mut tally = Tally::default();
    for k in 0..=calls {
        let disk = CrashDisk::from_image(image.clone());
        let store = crash_store(&disk, config(true));
        disk.crash_after(k);
        let _ = store.commit(&candidate);
        let image = disk.crash(store, CrashMode::AllUnsynced);
        let holds = check_image(
            image,
            config(true),
            &parents,
            &candidate,
            &mut tally,
            "monotone",
        );
        assert!(
            holds || !seen_candidate,
            "crash point {k} lost a published candidate"
        );
        seen_candidate |= holds;
    }
    assert!(seen_candidate);
}

/// Sampled crash points of a commit that spills pages before `commit()` and
/// grows the file.
#[test]
fn growing_commit_crash_points_are_parent_or_candidate() {
    let tally = run_growing(Some(&sampled(97)), 1);
    println!("growing, spilling commit: {tally:?}");
}

#[test]
#[ignore = "heavy: every crash point of a commit with thousands of backend calls"]
fn growing_commit_every_crash_point_heavy() {
    run_growing(None, 2);
    run_growing_steady(None, 2);
}

#[test]
#[ignore = "heavy: 64 torn subsets per granularity and crash point, steady state"]
fn every_crash_point_in_steady_state_heavy() {
    let tally = run_steady(true, HEAVY_SEEDS);
    assert_eq!(tally.full_repairs, 0);
    let tally = run_steady(false, HEAVY_SEEDS);
    assert!(tally.parent > 0 && tally.candidate > 0);
}
