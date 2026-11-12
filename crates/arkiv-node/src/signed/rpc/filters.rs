//! Installed log and block filters, polled by id.

use super::super::{ErrorCode, Result, host::Host};
use super::{
    RpcState, index,
    logs::{LOG_BLOCK_CAP, logs, validate_filter},
    params::{filter_height, first, quantity, uint},
};
use crate::text::hex;
use serde_json::{Value, json};
use std::time::{Duration, Instant};

const FILTER_CAP: usize = 1024;
pub(super) const FILTER_TTL: Duration = Duration::from_secs(15 * 60);

#[derive(Clone)]
pub(super) struct Filter {
    spec: Value,
    /// The first block the next `eth_getFilterChanges` reports.
    next: u64,
    /// Reports block hashes, not logs.
    blocks: bool,
    pub(super) touched: Instant,
}

fn expire_filters(state: &mut RpcState, now: Instant) {
    state
        .filters
        .retain(|_, f| now.saturating_duration_since(f.touched) < FILTER_TTL);
}

fn block_height(state: &RpcState, hash: &str) -> Result<u64> {
    Ok(*state
        .blocks
        .get(&hash.to_ascii_lowercase())
        .ok_or(ErrorCode::UnknownBlock)?)
}

/// `eth_newBlockFilter` when `blocks`, else `eth_newFilter`.
pub(super) fn new_filter(
    host: &Host,
    state: &mut RpcState,
    args: &[Value],
    blocks: bool,
) -> Result<Value> {
    let now = Instant::now();
    expire_filters(state, now);
    if state.filters.len() >= FILTER_CAP {
        return Err(ErrorCode::CapacityReached.into());
    }
    let mut spec = if blocks {
        json!({})
    } else {
        let spec = first(args)?;
        validate_filter(spec)?;
        spec.clone()
    };
    let next = if let Some(hash) = spec.get("blockHash").and_then(Value::as_str) {
        index(host, state)?;
        block_height(state, hash)?
    } else {
        let start = if spec.get("fromBlock").is_some() {
            filter_height(host.head.height, spec.get("fromBlock"))?
        } else {
            host.head
                .height
                .checked_add(1)
                .ok_or(ErrorCode::CapacityReached)?
        };
        // Freeze the creation-time starting point for getFilterLogs.
        spec["fromBlock"] = json!(quantity(start));
        start
    };
    state.next_filter = state
        .next_filter
        .checked_add(1)
        .ok_or(ErrorCode::CapacityReached)?;
    let id = state.next_filter;
    state.filters.insert(
        id,
        Filter {
            spec,
            next,
            blocks,
            touched: now,
        },
    );
    Ok(json!(quantity(id)))
}

pub(super) fn uninstall_filter(state: &mut RpcState, args: &[Value]) -> Result<Value> {
    expire_filters(state, Instant::now());
    let id = uint(first(args)?)?;
    Ok(json!(state.filters.remove(&id).is_some()))
}

/// The live filter a poll names, marked as used.
fn polled(state: &mut RpcState, args: &[Value]) -> Result<(u64, Filter)> {
    let now = Instant::now();
    expire_filters(state, now);
    let id = uint(first(args)?)?;
    let filter = state
        .filters
        .get_mut(&id)
        .ok_or(ErrorCode::FilterNotFound)?;
    filter.touched = now;
    Ok((id, filter.clone()))
}

pub(super) fn filter_logs(host: &Host, state: &mut RpcState, args: &[Value]) -> Result<Value> {
    let (
        _,
        Filter {
            mut spec, blocks, ..
        },
    ) = polled(state, args)?;
    if spec.get("blockHash").is_some() {
        return logs(host, state, &spec);
    }
    if blocks {
        return Err(ErrorCode::InvalidParams.into());
    }
    let head = host.head.height;
    let start = filter_height(head, spec.get("fromBlock"))?;
    let end = filter_height(head, spec.get("toBlock"))?.min(head);
    if start > end {
        return Ok(json!([]));
    }
    spec["fromBlock"] = json!(quantity(start));
    spec["toBlock"] = json!(quantity(end));
    logs(host, state, &spec)
}

pub(super) fn filter_changes(host: &Host, state: &mut RpcState, args: &[Value]) -> Result<Value> {
    let (id, filter) = polled(state, args)?;
    let Some((changes, last)) = changes_since(host, state, filter)? else {
        return Ok(json!([]));
    };
    state
        .filters
        .get_mut(&id)
        .ok_or(ErrorCode::FilterNotFound)?
        .next = last.checked_add(1).ok_or(ErrorCode::CapacityReached)?;
    Ok(changes)
}

/// What `filter` has to report and the height it is then read up to; `None`
/// when nothing is new.
fn changes_since(
    host: &Host,
    state: &mut RpcState,
    filter: Filter,
) -> Result<Option<(Value, u64)>> {
    let Filter {
        mut spec,
        next,
        blocks,
        ..
    } = filter;
    if spec.get("blockHash").is_some() {
        // A block-hash filter is a fixed one-shot change set. Never add
        // fromBlock/toBlock to this mutually exclusive selector.
        if next > 0 {
            let hash = spec["blockHash"].as_str().ok_or(ErrorCode::InvalidParams)?;
            if next > block_height(state, hash)? {
                return Ok(None);
            }
        }
        return Ok(Some((logs(host, state, &spec)?, next)));
    }
    let head = host.head.height;
    let bound = filter_height(head, spec.get("toBlock"))?.min(head);
    if next > bound {
        return Ok(None);
    }
    let end = bound.min(next.saturating_add(LOG_BLOCK_CAP - 1));
    let changes = if blocks {
        let hashes = (next..=end)
            .map(|height| Ok(hex(host.certificate(height)?.header.hash().as_slice())))
            .collect::<Result<Vec<_>>>()?;
        json!(hashes)
    } else {
        spec["fromBlock"] = json!(quantity(next));
        spec["toBlock"] = json!(quantity(end));
        logs(host, state, &spec)?
    };
    Ok(Some((changes, end)))
}
