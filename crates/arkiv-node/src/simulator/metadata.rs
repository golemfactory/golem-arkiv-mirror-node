//! One execution's bounded, terminal after-images. No payloads in these DTOs.

use super::wire::*;
use crate::{
    AdminOutcome, BlockInputs, BlockOutcomes, OpOutcome, TxOp, TxStatus, arkiv::ArkivOperation,
    exec::ExpiredRecord,
};
use arkiv_db::{
    Branch, Budget, CellKind, Effect, NamespaceId, Projection, RecordId, RecordKey, StorageError,
    StorageErrorKind, Value, digest, encode,
};
use std::collections::{BTreeMap, BTreeSet, btree_map::Entry};

#[derive(Default)]
pub struct Metadata {
    pub transactions: Vec<Transaction>,
    pub operations: Vec<Operation>,
    pub changes: Vec<Change>,
    pub spent: u64,
}

fn corrupt() -> StorageError {
    StorageError::new(
        StorageErrorKind::Corrupt,
        "metadata terminal state inconsistency",
    )
}

pub fn value_digest(v: &Value) -> String {
    hex(digest("arkiv/simulator-value/v1", &encode(v)).as_slice())
}

pub fn record_change(namespace: NamespaceId, row: arkiv_db::Row) -> super::Result<Change> {
    let mut attributes = Vec::new();
    let mut fields = Vec::new();
    let mut expiry = u64::MAX;
    for (name, cell) in row.cells {
        if cell.kind == CellKind::Attribute {
            if name.as_str() == crate::EXPIRES_AT
                && let Value::U64(n) = cell.value
            {
                expiry = n;
            }
            if matches!(cell.value, Value::Bytes(_)) {
                return Err(super::ErrorCode::StorageCorrupt.into());
            }
            let value = crate::value_json::to_json(&cell.value);
            attributes.push(Attribute {
                name: name.as_str().into(),
                value_type: cell.value.tag().name().into(),
                value,
            });
        } else {
            fields.push(Field {
                name: name.as_str().into(),
                value_type: cell.value.tag().name().into(),
                byte_length: cell.value.encoded_len().to_string(),
                digest: value_digest(&cell.value),
                reference: None,
            });
        }
    }
    Ok(Change::RecordUpsert {
        namespace_id: namespace.ordinal().to_string(),
        record_id: row.id.ordinal().to_string(),
        record_key: hex(row.key.ok_or(super::ErrorCode::StorageCorrupt)?.as_bytes()),
        expires_at_height: expiry.to_string(),
        attributes,
        fields,
    })
}

/// What the block's applied operations touched: the keys whose terminal
/// state the changes report.
#[derive(Default)]
struct Touched {
    namespaces: BTreeSet<NamespaceId>,
    records: BTreeMap<(NamespaceId, RecordId), RecordKey>,
    raw: BTreeSet<(NamespaceId, Vec<u8>)>,
}

pub fn capture(
    branch: &Branch,
    inputs: &BlockInputs,
    outcomes: &BlockOutcomes,
    expired: &[ExpiredRecord],
) -> std::result::Result<Metadata, StorageError> {
    let mut out = Metadata {
        spent: outcomes.spent,
        ..Default::default()
    };
    let mut touched = Touched::default();
    out.operations.extend(admin_operations(outcomes));
    out.operations
        .extend(expiry_operations(outcomes, expired, &mut touched)?);
    let mut create_nonces = starting_create_nonces(branch, inputs, outcomes)?;
    for (position, (tx, result)) in inputs.txs.iter().zip(&outcomes.txs).enumerate() {
        let position = position as u32;
        out.transactions.push(Transaction {
            position,
            digest: hex(tx.digest().as_slice()),
            actor: hex(tx.actor.as_bytes()),
            request_id: hex(&tx.request_id),
            budget_units: tx.tx_budget.to_string(),
            status: if result.status == TxStatus::Committed {
                "committed"
            } else {
                "failed"
            }
            .into(),
            spent_units: result.spent.to_string(),
            rent_wei: result.rent_wei.to_string(),
        });
        for (index, (op, result)) in tx.ops.iter().zip(&result.ops).enumerate() {
            out.operations.push(user_operation(
                (position, index as u32),
                tx,
                op,
                result,
                &mut create_nonces,
                &mut touched,
            )?);
        }
    }
    out.changes = changes(branch, touched)?;
    Ok(out)
}

fn admin_operations(outcomes: &BlockOutcomes) -> impl Iterator<Item = Operation> {
    outcomes.admin.iter().enumerate().map(|(i, result)| {
        let (outcome, reason) = match result {
            AdminOutcome::Applied => ("applied", None),
            AdminOutcome::Rejected(_) => ("rejected", Some("AdminRejected".into())),
        };
        Operation {
            phase: "admin".into(),
            group_position: i as u32,
            operation_position: 0,
            kind: "setPricing".into(),
            namespace_id: None,
            record_id: None,
            record_key: None,
            outcome: outcome.into(),
            receipt: None,
            reason,
        }
    })
}

fn expiry_operations(
    outcomes: &BlockOutcomes,
    expired: &[ExpiredRecord],
    touched: &mut Touched,
) -> std::result::Result<Vec<Operation>, StorageError> {
    if expired.len() != outcomes.expiry.len() {
        return Err(corrupt());
    }
    let mut operations = Vec::with_capacity(expired.len());
    for (i, (entry, result)) in expired.iter().zip(&outcomes.expiry).enumerate() {
        if (entry.namespace, entry.id) != (result.namespace, result.record_id) {
            return Err(corrupt());
        }
        touched
            .records
            .insert((entry.namespace, entry.id), entry.key.clone());
        operations.push(Operation {
            phase: "expiry".into(),
            group_position: 0,
            operation_position: i as u32,
            kind: "expireRecord".into(),
            namespace_id: Some(entry.namespace.ordinal().to_string()),
            record_id: Some(entry.id.ordinal().to_string()),
            record_key: Some(hex(entry.key.as_bytes())),
            outcome: "applied".into(),
            receipt: Some((&result.receipt).into()),
            reason: None,
        });
    }
    Ok(operations)
}

/// Each creator's creation nonce before the block. A committed nonce is
/// advanced once per successful create, so the starting nonces follow from
/// terminal state: even create-then-delete has a deterministic touched key
/// without storing a payload side channel.
fn starting_create_nonces(
    branch: &Branch,
    inputs: &BlockInputs,
    outcomes: &BlockOutcomes,
) -> std::result::Result<BTreeMap<[u8; 20], u64>, StorageError> {
    let mut creates: BTreeMap<[u8; 20], u64> = BTreeMap::new();
    for (tx, result) in inputs.txs.iter().zip(&outcomes.txs) {
        if result.status != TxStatus::Committed {
            continue;
        }
        let count = tx
            .ops
            .iter()
            .filter(|op| {
                matches!(
                    op,
                    TxOp::Arkiv {
                        operation: ArkivOperation::Create { .. },
                        ..
                    }
                )
            })
            .count() as u64;
        if count != 0 {
            *creates.entry(*tx.actor.as_bytes()).or_default() += count;
        }
    }
    for (actor, count) in &mut creates {
        let bytes = branch
            .try_host_get(&crate::arkiv::nonce_key(actor))?
            .ok_or_else(corrupt)?;
        let terminal = u64::from_be_bytes(bytes.try_into().map_err(|_| corrupt())?);
        *count = terminal.checked_sub(*count).ok_or_else(corrupt)?;
    }
    Ok(creates)
}

/// The key an operation addresses.
enum Addressed {
    Record(RecordKey),
    Raw(Vec<u8>),
}

/// The operation at `(transaction, operation)` position `at`, recording
/// what it touched if it applied.
fn user_operation(
    at: (u32, u32),
    tx: &crate::TxEnvelope,
    op: &TxOp,
    result: &OpOutcome,
    create_nonces: &mut BTreeMap<[u8; 20], u64>,
    touched: &mut Touched,
) -> std::result::Result<Operation, StorageError> {
    let (outcome, receipt, effect, reason) = match result {
        OpOutcome::Applied { effect, receipt } => ("applied", Some(receipt), Some(effect), None),
        OpOutcome::RolledBack { effect, receipt } => {
            ("rolledBack", Some(receipt), Some(effect), None)
        }
        OpOutcome::AccountRejected { receipt, .. } => {
            ("rejected", Some(receipt), None, Some("AccountRejected"))
        }
        OpOutcome::Rejected { receipt, .. } => {
            ("rejected", Some(receipt), None, Some("EngineRejected"))
        }
        OpOutcome::AdmissionRejected { .. } => {
            ("admissionRejected", None, None, Some("AdmissionRejected"))
        }
        OpOutcome::HostRejected { .. } => ("hostRejected", None, None, Some("HostRejected")),
        OpOutcome::NotExecuted => ("notExecuted", None, None, None),
    };
    let applied = matches!(result, OpOutcome::Applied { .. });
    let (namespace, record) = match effect {
        Some(Effect::NamespaceCreated(created)) => (Some(*created), None),
        Some(
            Effect::RecordCreated { id }
            | Effect::RecordPatched { id }
            | Effect::RecordDeleted { id },
        ) => (op.namespace(), Some(*id)),
        _ => (op.namespace(), None),
    };
    let addressed = match op {
        TxOp::Arkiv { operation, .. } => match operation {
            ArkivOperation::Create { salt, .. } if applied => {
                let nonce = create_nonces
                    .get_mut(tx.actor.as_bytes())
                    .ok_or_else(corrupt)?;
                let key = crate::arkiv::derive_entity_key(
                    tx.chain_id,
                    tx.actor.as_bytes(),
                    *nonce,
                    *salt,
                );
                *nonce = nonce.checked_add(1).ok_or_else(corrupt)?;
                Some(RecordKey::new(&key).ok_or_else(corrupt)?)
            }
            other => other.key().cloned(),
        }
        .map(Addressed::Record),
        TxOp::CreateRecord { key, .. }
        | TxOp::PatchRecord { key, .. }
        | TxOp::DeleteRecord { key, .. } => Some(Addressed::Record(key.clone())),
        TxOp::Put { key, .. } | TxOp::Delete { key, .. } => Some(Addressed::Raw(key.clone())),
        TxOp::Transfer { .. } | TxOp::CreateNamespace { .. } | TxOp::TransferNamespace { .. } => {
            None
        }
    };
    let kind = match op {
        TxOp::Transfer { .. } => "transfer",
        TxOp::Arkiv { operation, .. } => match operation {
            ArkivOperation::Create { .. } => "createRecord",
            ArkivOperation::Delete { .. } => "deleteRecord",
            _ => "patchRecord",
        },
        TxOp::CreateNamespace { .. } => "createNamespace",
        TxOp::TransferNamespace { .. } => "transferNamespace",
        TxOp::Put { .. } => "put",
        TxOp::Delete { .. } => "delete",
        TxOp::CreateRecord { .. } => "createRecord",
        TxOp::PatchRecord { .. } => "patchRecord",
        TxOp::DeleteRecord { .. } => "deleteRecord",
    };
    let record_key = addressed.as_ref().map(|addressed| match addressed {
        Addressed::Record(key) => hex(key.as_bytes()),
        Addressed::Raw(key) => hex(key),
    });
    if applied && let Some(namespace) = namespace {
        match (op, addressed) {
            (TxOp::CreateNamespace { .. } | TxOp::TransferNamespace { .. }, _) => {
                touched.namespaces.insert(namespace);
            }
            (_, Some(Addressed::Record(key))) => {
                touched
                    .records
                    .insert((namespace, record.ok_or_else(corrupt)?), key);
            }
            (_, Some(Addressed::Raw(key))) => {
                touched.raw.insert((namespace, key));
            }
            (_, None) => {}
        }
    }
    Ok(Operation {
        phase: "user".into(),
        group_position: at.0,
        operation_position: at.1,
        kind: kind.into(),
        namespace_id: namespace.map(|n| n.ordinal().to_string()),
        record_id: record.map(|n| n.ordinal().to_string()),
        record_key,
        outcome: outcome.into(),
        receipt: receipt.map(Into::into),
        reason: reason.map(Into::into),
    })
}

/// The terminal state of everything touched: namespaces, then records, then
/// raw keys.
fn changes(branch: &Branch, touched: Touched) -> std::result::Result<Vec<Change>, StorageError> {
    let mut changes = Vec::new();
    for namespace in touched.namespaces {
        let descriptor = branch
            .try_descriptor(namespace, Budget::unlimited())?
            .map_err(|_| corrupt())?
            .value;
        changes.push(Change::NamespaceUpsert {
            namespace_id: namespace.ordinal().to_string(),
            name: descriptor.name.as_str().into(),
            owner: hex(descriptor.owner.as_bytes()),
            revision: descriptor.revision.0.to_string(),
        });
    }
    // At most one terminal lookup per touched key, even for multiple incarnations.
    let mut terminal = BTreeMap::new();
    for (&(namespace, _), key) in &touched.records {
        if let Entry::Vacant(slot) = terminal.entry((namespace, key.clone())) {
            slot.insert(
                branch
                    .try_get_record(namespace, key, &Projection::ALL, Budget::unlimited())?
                    .map_err(|_| corrupt())?
                    .value,
            );
        }
    }
    for ((namespace, id), key) in touched.records {
        let row = terminal
            .get(&(namespace, key.clone()))
            .and_then(Option::as_ref);
        changes.push(match row {
            Some(row) if row.id == id => {
                record_change(namespace, row.clone()).map_err(|_| corrupt())?
            }
            _ => Change::RecordDelete {
                namespace_id: namespace.ordinal().to_string(),
                record_id: id.ordinal().to_string(),
                record_key: hex(key.as_bytes()),
            },
        });
    }
    for (namespace, key) in touched.raw {
        let value = branch
            .try_get(namespace, &key, Budget::unlimited())?
            .map_err(|_| corrupt())?
            .value;
        changes.push(match value {
            Some(value) => Change::RawUpsert {
                namespace_id: namespace.ordinal().to_string(),
                key: hex(&key),
                byte_length: value.len().to_string(),
                digest: value_digest(&Value::bytes(&value)),
            },
            None => Change::RawDelete {
                namespace_id: namespace.ordinal().to_string(),
                key: hex(&key),
            },
        });
    }
    Ok(changes)
}
