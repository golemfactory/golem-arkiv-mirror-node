//! Event logs, derived from block bodies and the state before each block.

use super::super::{Error, ErrorCode, Result, host::Host};
use super::{
    ADDRESS, RpcState,
    entities::{entity, entity_nonce},
    index,
    params::{at, bytes, filter_height, quantity, word},
};
use crate::{
    Expiry, TxOp, TxStatus,
    arkiv::{self, ArkivOperation},
    text::hex,
};
use alloy_primitives::keccak256;
use arkiv_db::RecordKey;
use serde_json::{Value, json};
use std::collections::BTreeMap;

pub(super) const LOG_BLOCK_CAP: u64 = 256;

/// Bound retained filters as well as reject silently ignored selector typos.
pub fn validate_filter(filter: &Value) -> Result<()> {
    let object = filter.as_object().ok_or(ErrorCode::InvalidParams)?;
    for (name, value) in object {
        match name.as_str() {
            "blockHash" => {
                bytes(value, Some(32))?;
            }
            "fromBlock" | "toBlock" => {
                filter_height(0, Some(value))?;
            }
            "address" => {
                if let Some(addresses) = value.as_array() {
                    if addresses.len() > 64 {
                        return Err(ErrorCode::InvalidParams.into());
                    }
                    for address in addresses {
                        bytes(address, Some(20))?;
                    }
                } else {
                    bytes(value, Some(20))?;
                }
            }
            "topics" => {
                let topics = value.as_array().ok_or(ErrorCode::InvalidParams)?;
                if topics.len() > 4 {
                    return Err(ErrorCode::InvalidParams.into());
                }
                for topic in topics {
                    if topic.is_null() {
                        continue;
                    }
                    if let Some(alternatives) = topic.as_array() {
                        if alternatives.len() > 64 {
                            return Err(ErrorCode::InvalidParams.into());
                        }
                        for alternative in alternatives {
                            if !alternative.is_null() {
                                bytes(alternative, Some(32))?;
                            }
                        }
                    } else {
                        bytes(topic, Some(32))?;
                    }
                }
            }
            _ => return Err(ErrorCode::InvalidParams.into()),
        }
    }
    if object.contains_key("blockHash")
        && (object.contains_key("fromBlock") || object.contains_key("toBlock"))
    {
        return Err(ErrorCode::InvalidParams.into());
    }
    Ok(())
}

/// The expiry heights the committed operations of one block resolved to,
/// replayed from the state before the block: a height that is full moves the
/// operation to the next with room ([`crate::exec::spill_expiry`]), so the
/// height an event reports is the one the entity was scheduled at, not
/// necessarily the one its operation asked for. Committed operations all
/// applied, in order, so the replay only follows the per-height totals.
struct ExpiryReplay<'a> {
    host: &'a Host,
    /// The state before the block.
    prior: arkiv_db::Snapshot,
    /// Changes of the per-height totals by the block's operations so far.
    totals: BTreeMap<u64, i64>,
    /// Expiries of the entities the block's operations touched so far.
    expiries: BTreeMap<Vec<u8>, Option<u64>>,
}

impl<'a> ExpiryReplay<'a> {
    fn new(host: &'a Host, height: u64) -> Result<Self> {
        Ok(Self {
            host,
            prior: host.state(height - 1)?,
            totals: BTreeMap::new(),
            expiries: BTreeMap::new(),
        })
    }

    fn expiry(&mut self, key: &RecordKey) -> Result<Option<u64>> {
        if let Some(known) = self.expiries.get(key.as_bytes()) {
            return Ok(*known);
        }
        Ok(entity(&self.prior, key)?.map(|entity| entity.expires_at))
    }

    fn shift(&mut self, height: Option<u64>, by: i64) {
        if let Some(h) = height.filter(|h| *h != u64::MAX) {
            *self.totals.entry(h).or_default() += by;
        }
    }

    /// Schedules `key` for the expiry `asked` at block `height` and returns
    /// the height it resolved to.
    fn set(&mut self, key: &[u8], asked: &Expiry, height: u64) -> Result<u64> {
        let target = asked.expires_at.max(
            height
                .checked_add(asked.min_lifetime)
                .ok_or(ErrorCode::StorageCorrupt)?,
        );
        let old = match RecordKey::new(key) {
            Some(key) => self.expiry(&key)?,
            None => None,
        };
        let resolved = if target == u64::MAX || old == Some(target) {
            target
        } else {
            let (prior, totals) = (&self.prior, &self.totals);
            crate::exec::spill_expiry(
                target,
                crate::exec::EXPIRY_SPILL_WINDOW,
                u64::MAX,
                self.host.genesis.genesis.params.expiry_per_block,
                |h| -> Result<u64> {
                    let stored = match prior.try_host_get(&crate::exec::total_key(h))? {
                        None => 0,
                        Some(b) => {
                            u64::from_be_bytes(b.try_into().map_err(|_| ErrorCode::StorageCorrupt)?)
                        }
                    };
                    let change = totals.get(&h).copied().unwrap_or(0);
                    u64::try_from(i128::from(stored) + i128::from(change))
                        .map_err(|_| Error::from(ErrorCode::StorageCorrupt))
                },
            )?
            .ok_or(ErrorCode::StorageCorrupt)?
        };
        if old != Some(resolved) {
            self.shift(old, -1);
            self.shift(Some(resolved), 1);
        }
        self.expiries.insert(key.to_vec(), Some(resolved));
        Ok(resolved)
    }

    fn delete(&mut self, key: &RecordKey) -> Result<()> {
        let old = self.expiry(key)?;
        self.shift(old, -1);
        self.expiries.insert(key.as_bytes().to_vec(), None);
        Ok(())
    }
}

/// An address as a 32-byte topic.
pub(super) fn topic_address(a: &[u8; 20]) -> String {
    format!("0x{}{}", "0".repeat(24), &hex(a)[2..])
}

struct Event {
    key: Vec<u8>,
    signature: &'static str,
    /// The indexed arguments after the entity key.
    topics: Vec<String>,
    data: String,
}

/// The entity state one block's committed operations changed so far, over
/// the state before the block.
struct BlockReplay<'a> {
    host: &'a Host,
    height: u64,
    /// Next creation nonce of each creator.
    nonces: BTreeMap<[u8; 20], u64>,
    owners: BTreeMap<Vec<u8>, [u8; 20]>,
    expiries: ExpiryReplay<'a>,
}

impl<'a> BlockReplay<'a> {
    fn new(host: &'a Host, height: u64) -> Result<Self> {
        Ok(Self {
            host,
            height,
            nonces: BTreeMap::new(),
            owners: BTreeMap::new(),
            expiries: ExpiryReplay::new(host, height)?,
        })
    }

    fn owner(&self, key: &RecordKey) -> Result<[u8; 20]> {
        match self.owners.get(key.as_bytes()) {
            Some(owner) => Ok(*owner),
            None => Ok(entity(&self.expiries.prior, key)?
                .ok_or(ErrorCode::StorageCorrupt)?
                .owner),
        }
    }

    /// Applies `operation` of `caller` and returns the event it emitted.
    fn event(&mut self, caller: [u8; 20], operation: &ArkivOperation) -> Result<Event> {
        let ArkivOperation::Create {
            salt,
            flags,
            expiry,
            ..
        } = operation
        else {
            return self.event_of_existing(operation);
        };
        let nonce = match self.nonces.get(&caller) {
            Some(nonce) => *nonce,
            None => entity_nonce(self.host, self.height - 1, &caller)?,
        };
        let key =
            arkiv::derive_entity_key(self.host.genesis.genesis.chain_id, &caller, nonce, *salt)
                .to_vec();
        self.nonces.insert(caller, nonce + 1);
        self.owners.insert(key.clone(), caller);
        let expiry = self.expiries.set(&key, expiry, self.height)?;
        Ok(Event {
            key,
            signature: "EntityCreated(bytes32,address,uint64,uint8)",
            topics: vec![topic_address(&caller)],
            data: format!("0x{}{}", word(expiry), word(*flags as u64)),
        })
    }

    fn event_of_existing(&mut self, operation: &ArkivOperation) -> Result<Event> {
        let key = operation.key().ok_or(ErrorCode::StorageCorrupt)?;
        let mut topics = vec![topic_address(&self.owner(key)?)];
        let mut data = "0x".to_owned();
        let signature = match operation {
            ArkivOperation::Create { .. } => return Err(ErrorCode::StorageCorrupt.into()),
            ArkivOperation::Patch { .. } => "EntityPatched(bytes32,address)",
            ArkivOperation::ExtendExpiry { expiry, .. } => {
                let expiry = self.expiries.set(key.as_bytes(), expiry, self.height)?;
                data = format!("0x{}", word(expiry));
                "ExpiryExtended(bytes32,address,uint64)"
            }
            ArkivOperation::TransferOwnership { owner, .. } => {
                topics.push(topic_address(owner));
                self.owners.insert(key.as_bytes().to_vec(), *owner);
                "OwnershipTransferred(bytes32,address,address)"
            }
            ArkivOperation::Delete { .. } => {
                self.expiries.delete(key)?;
                "EntityDeleted(bytes32,address)"
            }
        };
        Ok(Event {
            key: key.as_bytes().to_vec(),
            signature,
            topics,
            data,
        })
    }
}

/// Logs of block `height`, derived from its body and the state after
/// `height - 1`; refused up front below the receipt floor (see
/// `crate::retention`), whether or not the block has Arkiv operations.
pub fn block_logs(host: &Host, height: u64) -> Result<Vec<Value>> {
    host.require_receipts(height)?;
    if height == 0 {
        return Ok(vec![]);
    }
    let block = host.shared_block(height)?;
    let mut replay = BlockReplay::new(host, height)?;
    let mut logs = Vec::new();
    // Hashed once per block and per transaction, not per log: a transaction
    // hash covers its whole body.
    let block_hash = hex(block.hash().as_slice());
    let committed = block
        .inputs
        .txs
        .iter()
        .zip(&block.outcomes.txs)
        .enumerate()
        .filter(|(_, (_, outcome))| outcome.status == TxStatus::Committed);
    for (tx_index, (tx, _)) in committed {
        let tx_hash = hex(tx.hash().as_slice());
        for op in &tx.tx.ops {
            let TxOp::Arkiv { operation, .. } = op else {
                continue;
            };
            let event = replay.event(*tx.tx.actor.as_bytes(), operation)?;
            let mut topics = vec![
                hex(keccak256(event.signature.as_bytes()).as_slice()),
                hex(&event.key),
            ];
            topics.extend(event.topics);
            logs.push(json!({
                "address": ADDRESS,
                "topics": topics,
                "data": event.data,
                "blockNumber": quantity(height),
                "transactionHash": tx_hash,
                "transactionIndex": quantity(tx_index as u64),
                "blockHash": block_hash,
                "logIndex": quantity(logs.len() as u64),
                "removed": false,
            }));
        }
    }
    Ok(logs)
}

pub(super) fn bloom(logs: &[Value]) -> String {
    let mut bloom = [0u8; 256];
    for l in logs {
        for s in std::iter::once(l["address"].as_str())
            .chain(
                l["topics"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .map(Value::as_str),
            )
            .flatten()
        {
            if let Ok(b) = bytes(&json!(s), None) {
                let h = keccak256(b);
                for i in [0, 2, 4] {
                    let bit = (((h[i] as usize) << 8) | h[i + 1] as usize) & 2047;
                    bloom[255 - bit / 8] |= 1 << (bit % 8);
                }
            }
        }
    }
    hex(&bloom)
}

/// Whether `selector`, one value or an array of alternatives, names `actual`.
fn selects(selector: &Value, actual: &str) -> bool {
    let is = |s: &Value| s.as_str().is_some_and(|s| s.eq_ignore_ascii_case(actual));
    is(selector) || selector.as_array().is_some_and(|v| v.iter().any(is))
}

fn matches_log(log: &Value, filter: &Value) -> bool {
    if let Some(address) = filter.get("address")
        && !selects(address, log["address"].as_str().unwrap_or_default())
    {
        return false;
    }
    let Some(topics) = filter.get("topics").and_then(Value::as_array) else {
        return true;
    };
    if topics.len() > log["topics"].as_array().map_or(0, Vec::len) {
        return false;
    }
    topics.iter().enumerate().all(|(i, topic)| {
        // A null position, or a null among its alternatives, matches any topic.
        topic.is_null()
            || topic
                .as_array()
                .is_some_and(|v| v.iter().any(Value::is_null))
            || selects(topic, log["topics"][i].as_str().unwrap_or_default())
    })
}

pub(super) fn logs(host: &Host, state: &mut RpcState, filter: &Value) -> Result<Value> {
    validate_filter(filter)?;
    let (start, end) = if let Some(hash) = filter.get("blockHash").and_then(Value::as_str) {
        index(host, state)?;
        let h = *state
            .blocks
            .get(&hash.to_ascii_lowercase())
            .ok_or(ErrorCode::UnknownBlock)?;
        (h, h)
    } else {
        (
            at(host, filter.get("fromBlock"))?,
            at(host, filter.get("toBlock"))?,
        )
    };
    if end < start || end - start >= LOG_BLOCK_CAP {
        return Err(ErrorCode::LogRangeTooLarge.into());
    }
    // The whole range is refused up front; the floor only rises.
    host.require_receipts(start)?;
    let mut out = Vec::new();
    for h in start..=end {
        for l in block_logs(host, h)? {
            if matches_log(&l, filter) {
                out.push(l);
            }
        }
    }
    Ok(json!(out))
}
