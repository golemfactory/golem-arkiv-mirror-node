use std::sync::Arc;

use arkiv_db_trie::Probe;

use crate::{
    layout::{Blob, NamespaceState, RawKey, Records},
    meter::{Budget, Cost, Meter, OperationClass, Outcome, PriceSchedule, ReceiptKind, Rejection},
    namespace::{NamespaceDescriptor, NamespaceId, Revision},
    nodes::{Fault, Nodes},
    record::Limits,
    typed::{self, TypedState},
};

use super::{
    WorkingState, checked,
    ops::{Authority, Effect, Rejected, WriteOp},
};

/// A zero-cost rejection before any charge.
pub(super) fn zero_cost(
    pricing: &PriceSchedule,
    budget: Budget,
    kind: ReceiptKind,
    reason: Rejection,
) -> Rejected {
    let (receipt, physical) =
        Meter::new(pricing, budget, None).finish(kind, Outcome::Rejected(reason));
    Rejected {
        reason,
        receipt,
        physical,
    }
}

/// The admission checks shared by execution and estimation: the host-path
/// restriction, argument validation, then the block-budget check.
pub(super) fn admit(
    pricing: &PriceSchedule,
    limits: &Limits,
    authority: Authority,
    op: &WriteOp,
    budget: Budget,
    remaining: Cost,
) -> Option<Rejected> {
    if authority == Authority::Host
        && !matches!(
            op,
            WriteOp::CreateRecord { .. }
                | WriteOp::PatchRecord { .. }
                | WriteOp::DeleteRecord { .. }
        )
    {
        let reason = Rejection::InvalidArgument(crate::record::ArgumentFault::NotHostOperation);
        return Some(zero_cost(pricing, budget, ReceiptKind::Write, reason));
    }
    if let Err(reason) = op.validate(limits) {
        return Some(zero_cost(pricing, budget, ReceiptKind::Write, reason));
    }
    let limit = budget.limit_units()?;
    if limit <= remaining {
        return None;
    }
    let reason = Rejection::ExceedsBlockBudget { remaining };
    Some(zero_cost(pricing, budget, ReceiptKind::Write, reason))
}

pub(super) fn units(length: usize) -> u64 {
    u64::try_from(length).expect("lengths fit in 64 bits")
}

pub(super) fn raw_key(key: &[u8]) -> RawKey {
    RawKey(Arc::from(key))
}

/// Resolves a namespace through the directory, recording the physical work.
pub(super) fn resolve(
    state: &WorkingState,
    nodes: &Nodes,
    namespace: NamespaceId,
    meter: &mut Meter<'_>,
) -> Result<NamespaceState, Fault> {
    let mut probe = Probe::new();
    let found = state.catalog.get(&namespace, &nodes.catalog(), &mut probe);
    meter.note_directory_nodes(probe.nodes);
    let leaf = found?.ok_or(Rejection::UnknownNamespace)?;
    Ok(checked(leaf, namespace)?)
}

fn authorize(state: &NamespaceState, authority: Authority) -> Result<(), Rejection> {
    match authority {
        Authority::Host => Ok(()),
        Authority::Owner(actor) if state.descriptor.owner == actor => Ok(()),
        Authority::Owner(_) => Err(Rejection::Unauthorized),
    }
}

/// Stores an updated namespace state in the directory.
fn commit_namespace(
    state: &mut WorkingState,
    nodes: &Nodes,
    namespace: NamespaceId,
    updated: NamespaceState,
    meter: &mut Meter<'_>,
) -> Result<(), Fault> {
    let mut probe = Probe::new();
    let inserted = state
        .catalog
        .insert(&namespace, updated, &nodes.catalog(), &mut probe);
    meter.note_directory_nodes(probe.nodes);
    let (catalog, _) = inserted?;
    state.catalog = catalog;
    Ok(())
}

/// Applies one write to `state`, charging every unit before its work.
///
/// Rejections are checked in a fixed order after the directory charge:
/// unknown namespace, then authorization, then revision, then the
/// operation's own charges. A storage error may leave `state` partly
/// updated; the caller restores it.
pub(super) fn apply(
    state: &mut WorkingState,
    nodes: &Nodes,
    limits: &Limits,
    authority: Authority,
    op: WriteOp,
    meter: &mut Meter<'_>,
) -> Result<Effect, Fault> {
    match op {
        WriteOp::CreateNamespace { name } => {
            meter.charge(OperationClass::DirectoryLookup, 1)?;
            if state.catalog.len() >= limits.namespace_cap {
                return Err(Rejection::DirectoryFull.into());
            }
            let Authority::Owner(actor) = authority else {
                return Err(Rejection::Unauthorized.into());
            };
            // One logical entry write: the new catalog leaf.
            meter.charge_group(&[
                (OperationClass::NamespaceCreate, 1),
                (OperationClass::RootUpdate, 1),
            ])?;
            let id = NamespaceId::from_ordinal(state.next_namespace)
                .ok_or(Rejection::CounterExhausted)?;
            let next = state
                .next_namespace
                .checked_add(1)
                .ok_or(Rejection::CounterExhausted)?;
            let created = NamespaceState {
                descriptor: NamespaceDescriptor {
                    id,
                    name,
                    owner: actor,
                    revision: Revision::INITIAL,
                },
                records: Records::new(),
                typed: TypedState::new(),
            };
            commit_namespace(state, nodes, id, created, meter)?;
            state.next_namespace = next;
            Ok(Effect::NamespaceCreated(id))
        }
        WriteOp::TransferNamespace {
            namespace,
            expected_revision,
            new_owner,
        } => {
            meter.charge(OperationClass::DirectoryLookup, 1)?;
            let existing = resolve(state, nodes, namespace, meter)?;
            authorize(&existing, authority)?;
            let current = existing.descriptor.revision;
            if current != expected_revision {
                return Err(Rejection::StaleRevision {
                    expected: expected_revision,
                    current,
                }
                .into());
            }
            // One logical entry write: the replaced catalog leaf.
            meter.charge_group(&[
                (OperationClass::NamespaceTransfer, 1),
                (OperationClass::RootUpdate, 1),
            ])?;
            let revision = Revision(
                current
                    .0
                    .checked_add(1)
                    .ok_or(Rejection::CounterExhausted)?,
            );
            // The record maps are shared as-is: no record is visited or copied.
            let updated = NamespaceState {
                descriptor: NamespaceDescriptor {
                    id: namespace,
                    name: existing.descriptor.name,
                    owner: new_owner,
                    revision,
                },
                ..existing
            };
            commit_namespace(state, nodes, namespace, updated, meter)?;
            Ok(Effect::Transferred { revision })
        }
        WriteOp::Put {
            namespace,
            key,
            value,
        } => {
            meter.charge(OperationClass::DirectoryLookup, 1)?;
            let existing = resolve(state, nodes, namespace, meter)?;
            authorize(&existing, authority)?;
            // Two logical entry writes: the raw entry and the catalog leaf.
            meter.charge_group(&[
                (OperationClass::PutRecord, 1),
                (OperationClass::WriteKeyByte, units(key.len())),
                (OperationClass::WriteValueByte, units(value.len())),
                (OperationClass::RootUpdate, 2),
            ])?;
            let mut probe = Probe::new();
            let inserted = existing.records.insert(
                &raw_key(&key),
                Blob(Arc::from(value)),
                &nodes.namespace(namespace).records,
                &mut probe,
            );
            meter.note_record_nodes(probe.nodes);
            let (records, previous) = inserted?;
            let updated = NamespaceState {
                records,
                ..existing
            };
            commit_namespace(state, nodes, namespace, updated, meter)?;
            Ok(Effect::Put {
                replaced: previous.is_some(),
            })
        }
        WriteOp::Delete { namespace, key } => {
            meter.charge(OperationClass::DirectoryLookup, 1)?;
            let existing = resolve(state, nodes, namespace, meter)?;
            authorize(&existing, authority)?;
            // Two logical entry writes (the raw entry and the catalog leaf),
            // charged whether or not the key exists so the charge does not
            // depend on the lookup.
            meter.charge_group(&[
                (OperationClass::DeleteRecord, 1),
                (OperationClass::WriteKeyByte, units(key.len())),
                (OperationClass::RootUpdate, 2),
            ])?;
            let mut probe = Probe::new();
            let removed = existing.records.remove(
                &raw_key(&key),
                &nodes.namespace(namespace).records,
                &mut probe,
            );
            meter.note_record_nodes(probe.nodes);
            let (records, previous) = removed?;
            let updated = NamespaceState {
                records,
                ..existing
            };
            commit_namespace(state, nodes, namespace, updated, meter)?;
            Ok(Effect::Deleted {
                existed: previous.is_some(),
            })
        }
        WriteOp::CreateRecord {
            namespace,
            key,
            cells,
        } => {
            meter.charge(OperationClass::DirectoryLookup, 1)?;
            let existing = resolve(state, nodes, namespace, meter)?;
            authorize(&existing, authority)?;
            let loaders = nodes.namespace(namespace);
            let (typed, id) = typed::create(&existing.typed, &loaders, key, cells, meter)?;
            let updated = NamespaceState { typed, ..existing };
            commit_namespace(state, nodes, namespace, updated, meter)?;
            Ok(Effect::RecordCreated { id })
        }
        WriteOp::PatchRecord {
            namespace,
            key,
            set,
            remove,
        } => {
            meter.charge(OperationClass::DirectoryLookup, 1)?;
            let existing = resolve(state, nodes, namespace, meter)?;
            authorize(&existing, authority)?;
            let loaders = nodes.namespace(namespace);
            let (typed, id) =
                typed::patch(&existing.typed, &loaders, limits, &key, set, remove, meter)?;
            let updated = NamespaceState { typed, ..existing };
            commit_namespace(state, nodes, namespace, updated, meter)?;
            Ok(Effect::RecordPatched { id })
        }
        WriteOp::DeleteRecord { namespace, key } => {
            meter.charge(OperationClass::DirectoryLookup, 1)?;
            let existing = resolve(state, nodes, namespace, meter)?;
            authorize(&existing, authority)?;
            let loaders = nodes.namespace(namespace);
            let (typed, id) = typed::delete(&existing.typed, &loaders, &key, meter)?;
            let updated = NamespaceState { typed, ..existing };
            commit_namespace(state, nodes, namespace, updated, meter)?;
            Ok(Effect::RecordDeleted { id })
        }
    }
}
