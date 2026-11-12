//! Host-side helpers that are deliberately outside the engine.
//!
//! The host owns consensus, finality and the mapping from block numbers to
//! engine snapshots. A block may be produced by several publications (for
//! example an admin stage and then a data stage), so block numbers and
//! snapshot IDs are distinct sequences.

use std::{fmt, ops::Bound};

use arkiv_db_storage::versioned::StorageError;

use crate::{
    engine::{Branch, Rejected, WriteOp, in_memory},
    meter::{Budget, Receipt},
    namespace::NamespaceId,
    query::{Projection, Query, Term},
    record::{CellName, RecordId, TypeTag, Value},
};

/// A block height as assigned by the host chain.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct BlockNumber(pub u64);

impl fmt::Display for BlockNumber {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "block#{}", self.0)
    }
}

/// An expiry run that stopped at a rejection. The deletes applied before it
/// stay applied in the branch and keep their receipts there.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExpiryHalt {
    /// The deletes applied before the rejection, in id order.
    pub applied: Vec<(RecordId, Receipt)>,
    /// The query or delete rejection that stopped the run.
    pub rejected: Rejected,
}

impl fmt::Display for ExpiryHalt {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "expiry halted after {} deletes: {}",
            self.applied.len(),
            self.rejected
        )
    }
}

impl std::error::Error for ExpiryHalt {}

/// Deletes, with host authority, every live record of `namespace` whose
/// `name` attribute is a `U64` no greater than `block`: the host's expiry
/// step for a block, run on the block's branch before it is sealed.
///
/// The engine has no clock: expiry is this deletion in the block where the
/// record expires, so snapshot(B) lacks the record while snapshot(B-1) keeps
/// it, and no read consults the head. The host stores the expiry height as
/// an ordinary `U64` attribute and must keep users from writing it (the
/// engine assigns `$`-names no meaning).
///
/// The sweep goes height by height in ascending order: `terms` reads the
/// smallest live height (`query_budget`; one seek and one term), then the
/// rows holding that height are read in pages of at most `max_page`
/// (`query_budget`, an `Eq` posting positioned from its start) and deleted
/// one by one with `delete_budget`, until the height has no rows and the
/// next smallest height is read. Read receipts are discarded. Deletes are
/// reported in (height, id) order. Every read is positional, so the work is
/// linear in the distinct expired heights plus the deleted rows, and a
/// backlog of heights (after a halted run) is caught up one height per
/// iteration rather than re-scanned per page.
///
/// Any rejection stops the run with the deletes applied so far, which
/// remain applied in the branch. The host must reserve block budget for
/// expiry (run it first, or size the block budget for it); a halted run
/// leaves the remaining rows live until a later block, where the smallest
/// live height is read first again.
#[expect(clippy::result_large_err)]
pub fn expire_records(
    branch: &mut Branch,
    namespace: NamespaceId,
    name: &CellName,
    block: BlockNumber,
    query_budget: Budget,
    delete_budget: Budget,
) -> Result<Vec<(RecordId, Receipt)>, ExpiryHalt> {
    in_memory(try_expire_records(
        branch,
        namespace,
        name,
        block,
        query_budget,
        delete_budget,
    ))
}

/// Fallible form of [`expire_records`], built on the branch's `try_` reads
/// and host deletes (see
/// [Fallible surface](crate::engine#fallible-surface)).
///
/// The outer `Err` is a storage failure: the sweep stopped without a
/// deterministic outcome and the host must discard the branch, never record
/// a partial expiry. The inner result is exactly what [`expire_records`]
/// returns. The in-memory engine never fails.
pub fn try_expire_records(
    branch: &mut Branch,
    namespace: NamespaceId,
    name: &CellName,
    block: BlockNumber,
    query_budget: Budget,
    delete_budget: Budget,
) -> Result<Result<Vec<(RecordId, Receipt)>, ExpiryHalt>, StorageError> {
    try_expire_records_observed(
        branch,
        namespace,
        name,
        block,
        query_budget,
        delete_budget,
        &mut |_, _, _| Ok(()),
    )
}

/// Expiry with a synchronous, noncanonical observer of successful deletions.
/// The key comes from the existing query; observing adds no reads or charges.
/// An observer failure requires discarding the entire branch, just like an I/O failure.
/// Hosts must bound any collection they retain in the observer.
pub fn try_expire_records_observed(
    branch: &mut Branch,
    namespace: NamespaceId,
    name: &CellName,
    block: BlockNumber,
    query_budget: Budget,
    delete_budget: Budget,
    observer: &mut dyn FnMut(RecordId, &crate::RecordKey, &Receipt) -> Result<(), StorageError>,
) -> Result<Result<Vec<(RecordId, Receipt)>, ExpiryHalt>, StorageError> {
    let limit = branch.limits().max_page;
    let mut applied = Vec::new();
    loop {
        let heights = match branch.try_terms(
            namespace,
            name,
            TypeTag::U64,
            Bound::Unbounded,
            1,
            query_budget,
        )? {
            Ok(heights) => heights.value,
            Err(rejected) => return Ok(Err(ExpiryHalt { applied, rejected })),
        };
        let height = match heights.first() {
            Some(Value::U64(height)) if *height <= block.0 => *height,
            _ => return Ok(Ok(applied)),
        };
        let due = Query::term(Term::eq(name.clone(), Value::U64(height)));
        loop {
            let page = match branch.try_query(
                namespace,
                &due,
                &Projection::KEY_ONLY,
                limit,
                None,
                query_budget,
            )? {
                Ok(page) => page.value,
                Err(rejected) => return Ok(Err(ExpiryHalt { applied, rejected })),
            };
            if page.rows.is_empty() {
                break;
            }
            for row in page.rows {
                let key = row.key.expect("the key was projected");
                let op = WriteOp::DeleteRecord {
                    namespace,
                    key: key.clone(),
                };
                match branch.try_execute_host(op, delete_budget)? {
                    Ok(executed) => {
                        observer(row.id, &key, &executed.receipt)?;
                        applied.push((row.id, executed.receipt));
                    }
                    Err(rejected) => return Ok(Err(ExpiryHalt { applied, rejected })),
                }
            }
        }
    }
}
