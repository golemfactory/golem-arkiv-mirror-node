//! JSON-RPC parameters and the quantities responses carry.

use super::super::{ErrorCode, Result, host::Host, protocol::TX_CAP};
use arkiv_db::OwnerId;
use serde_json::Value;

pub(super) fn quantity(n: u64) -> String {
    format!("0x{n:x}")
}

/// A `u64` as one 32-byte ABI word, without the `0x`.
pub(super) fn word(n: u64) -> String {
    format!("{n:064x}")
}

pub(super) fn args(params: &Value) -> Result<&[Value]> {
    Ok(params.as_array().ok_or(ErrorCode::InvalidParams)?)
}

pub(super) fn first(args: &[Value]) -> Result<&Value> {
    Ok(args.first().ok_or(ErrorCode::InvalidParams)?)
}

pub(super) fn uint(v: &Value) -> Result<u64> {
    v.as_str()
        .and_then(crate::text::quantity)
        .ok_or(ErrorCode::InvalidParams.into())
}

/// Hex of either case, of exactly `len` bytes when one is given.
pub(super) fn bytes(v: &Value, len: Option<usize>) -> Result<Vec<u8>> {
    let s = v.as_str().ok_or(ErrorCode::InvalidParams)?;
    let s = s.strip_prefix("0x").ok_or(ErrorCode::InvalidParams)?;
    if !s.bytes().all(|b| b.is_ascii_hexdigit())
        || s.len() % 2 != 0
        || s.len() > 2 * TX_CAP as usize
        || len.is_some_and(|l| s.len() != l * 2)
    {
        return Err(ErrorCode::InvalidParams.into());
    }
    alloy_primitives::hex::decode(s).map_err(|_| ErrorCode::InvalidParams.into())
}

pub(super) fn actor(v: &Value) -> Result<OwnerId> {
    Ok(OwnerId::new(
        bytes(v, Some(20))?
            .try_into()
            .map_err(|_| ErrorCode::InvalidParams)?,
    ))
}

/// A block tag or number; an absent one is the head. Numeric filter bounds
/// may point beyond the current head: they stay fixed while
/// latest/pending/safe/finalized follow the head on each poll.
pub(super) fn filter_height(head: u64, value: Option<&Value>) -> Result<u64> {
    let Some(value) = value else {
        return Ok(head);
    };
    match value.as_str().ok_or(ErrorCode::InvalidParams)? {
        "latest" | "pending" | "safe" | "finalized" => Ok(head),
        "earliest" => Ok(0),
        _ => uint(value),
    }
}

/// The height of an existing block.
pub(super) fn at(host: &Host, v: Option<&Value>) -> Result<u64> {
    let height = filter_height(host.head.height, v)?;
    if height > host.head.height {
        return Err(ErrorCode::UnknownBlock.into());
    }
    Ok(height)
}
