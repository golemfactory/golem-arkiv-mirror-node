//! Node-side state-retention checks shared by the signed and simulator hosts.
//!
//! The durable engine refuses snapshots below its state floor
//! (`arkiv_db::durable::Retention`). Node read paths check the floor up front,
//! before opening a snapshot; the refusal names the requested height and the
//! floor:
//!
//! - state at height `h` (balances, nonces, proofs, queries, the state tree
//!   explorer) needs `h >= stateFloor`;
//! - receipts, logs and the `logsBloom` of block `h > 0` are derived from body
//!   `h` and state `h - 1`, so they need `h >= receiptFloor`, where
//!   `receiptFloor = stateFloor + 1` when `stateFloor > 0` and `0` otherwise.
//!   Block 0 has no logs. A receipt refusal reports the block height and the
//!   receipt floor.
//!
//! Headers, bodies, feed records, inclusions and the hash index are retained
//! in full and never refused.

use crate::error::{Error, Result};
use arkiv_db::{Snapshot, SnapshotId, durable::DurableEngine};
use serde_json::{Value as Json, json};

/// The lowest height whose state is served.
pub fn state_floor(engine: &DurableEngine) -> u64 {
    engine.state_floor().sequence()
}

/// The lowest height whose receipts/logs are served (see module docs).
pub fn receipt_floor(engine: &DurableEngine) -> u64 {
    match state_floor(engine) {
        0 => 0,
        floor => floor + 1,
    }
}

/// Refuses state at `height` below the floor. Heights above the head are
/// left to the caller's own unknown-block handling.
pub fn require_state(engine: &DurableEngine, height: u64) -> Result<()> {
    let floor = state_floor(engine);
    if height < floor {
        return Err(Error::pruned(height, floor));
    }
    Ok(())
}

/// Refuses receipts/logs of block `height` below the receipt floor.
pub fn require_receipts(engine: &DurableEngine, height: u64) -> Result<()> {
    let floor = receipt_floor(engine);
    if height > 0 && height < floor {
        return Err(Error::pruned(height, floor));
    }
    Ok(())
}

/// Opens the snapshot at `height` after the up-front floor check; the
/// engine's own refusal is the backstop.
pub fn state(engine: &DurableEngine, height: u64) -> Result<Snapshot> {
    require_state(engine, height)?;
    Ok(engine.snapshot(SnapshotId::from_sequence(height))?)
}

/// Status fields describing the served state history.
pub fn status(engine: &DurableEngine) -> Json {
    let retention = engine.retention_status().retention;
    json!({
        "stateRetention": retention.mode(),
        "stateRetentionWindow": retention.window().map(|w| w.to_string()),
        "stateFloor": state_floor(engine).to_string(),
        "receiptFloor": receipt_floor(engine).to_string(),
    })
}

/// The REST error body: `{"error":"HistoryPruned","requestedHeight":"N","stateFloor":"F"}`
/// (details when known), or `{"error":code}`.
pub fn error_body(e: &Error) -> Json {
    match e.pruned_at() {
        Some((requested, floor)) => json!({
            "error": e.code.as_str(),
            "requestedHeight": requested.to_string(),
            "stateFloor": floor.to_string(),
        }),
        None => json!({ "error": e.code.as_str() }),
    }
}
