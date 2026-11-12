//! Arkiv entity state as the log and revert replays read it.

use super::super::{ErrorCode, Result, host::Host, rpc_reads::READ_BUDGET};
use crate::arkiv::{self, ArkivEntityProjection};
use arkiv_db::{Budget, Projection, RecordKey, Snapshot};

pub(super) fn entity(
    snapshot: &Snapshot,
    key: &RecordKey,
) -> Result<Option<ArkivEntityProjection>> {
    let record = snapshot
        .try_get_record(
            arkiv::entity_namespace(),
            key,
            &Projection::ALL,
            Budget::limit(READ_BUDGET),
        )?
        .map_err(|_| ErrorCode::StorageCorrupt)?
        .value;
    Ok(record
        .map(|record| arkiv::project(&record).map_err(|_| ErrorCode::StorageCorrupt))
        .transpose()?)
}

/// The creation nonce of `address` in the state after block `height`.
pub(super) fn entity_nonce(host: &Host, height: u64, address: &[u8; 20]) -> Result<u64> {
    match host
        .state(height)?
        .try_host_get(&arkiv::nonce_key(address))?
    {
        None => Ok(0),
        Some(stored) => Ok(u64::from_be_bytes(
            stored.try_into().map_err(|_| ErrorCode::StorageCorrupt)?,
        )),
    }
}
