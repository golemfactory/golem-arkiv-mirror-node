//! Versioned Arkiv entity host. Authority and counters live in committed engine state.

mod schema;
use crate::{ExecError, Expiry, HostFault, OpOutcome, exec::Lifetime};
use arkiv_db::{
    Branch, Budget, Cell, CellName, Decode, DecodeError, Decoder, Encode, Encoder, HostKey,
    NamespaceId, Projection, RecordKey, Value, WriteOp,
};
pub use schema::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum ArkivFault {
    Name = 1,
    Value = 2,
    Flags = 3,
    NotFound = 4,
    Expired = 5,
    NotOwner = 6,
    Readonly = 7,
    Owner = 8,
    NonceExhausted = 9,
    Corrupt = 10,
    Namespace = 11,
    Duplicate = 12,
    GasOverflow = 13,
    /// Reserved tag, never produced.
    PermanentStorageUnsupported = 14,
    UnsupportedGasOperation = 15,
    /// The `bytes` cells of the resulting entity would hold more than
    /// [`MAX_ENTITY_BYTES`].
    EntityTooLarge = 16,
    /// The sender's balance, after its transaction's gas reservation, cannot
    /// pay the operation's rent. Nothing of the operation applies.
    InsufficientRentBalance = 17,
    /// A permissionless extension (creation flag 2) by someone other than
    /// the owner asks for more than one rent cap ahead
    /// (`height + cap_blocks`): further expiries, permanence included, are
    /// the owner's alone.
    PermanenceOwnerOnly = 18,
}

impl ArkivFault {
    const ALL: [Self; 18] = [
        Self::Name,
        Self::Value,
        Self::Flags,
        Self::NotFound,
        Self::Expired,
        Self::NotOwner,
        Self::Readonly,
        Self::Owner,
        Self::NonceExhausted,
        Self::Corrupt,
        Self::Namespace,
        Self::Duplicate,
        Self::GasOverflow,
        Self::PermanentStorageUnsupported,
        Self::UnsupportedGasOperation,
        Self::EntityTooLarge,
        Self::InsufficientRentBalance,
        Self::PermanenceOwnerOnly,
    ];
}

/// The fault's discriminant, one byte.
impl Encode for ArkivFault {
    fn encode_to(&self, encoder: &mut Encoder) {
        encoder.u8(*self as u8)
    }
}

impl Decode for ArkivFault {
    const MIN_ENCODED_LEN: usize = 1;
    fn decode_from(decoder: &mut Decoder<'_>) -> Result<Self, DecodeError> {
        let tag = decoder.u8()?;
        Self::ALL
            .into_iter()
            .find(|fault| *fault as u8 == tag)
            .ok_or(DecodeError::UnknownTag {
                kind: "ArkivFault",
                tag,
            })
    }
}

/// One entity operation. User attributes travel as engine cells: an indexed
/// attribute (ABI `typeId` with [`INDEXED_FLAG`]) is a [`Cell::attribute`],
/// an unindexed one a [`Cell::field`], so the flag is carried, validated,
/// executed and stored as the cell kind.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ArkivOperation {
    Create {
        salt: u128,
        flags: u8,
        expiry: Expiry,
        attributes: Vec<(CellName, Cell)>,
    },
    /// Sets (each with the indexed flag of its new triple) and removes
    /// attributes.
    Patch {
        key: RecordKey,
        set: Vec<(CellName, Cell)>,
        remove: Vec<CellName>,
    },
    ExtendExpiry {
        key: RecordKey,
        expiry: Expiry,
    },
    TransferOwnership {
        key: RecordKey,
        owner: [u8; 20],
    },
    Delete {
        key: RecordKey,
    },
}

impl ArkivOperation {
    pub fn key(&self) -> Option<&RecordKey> {
        match self {
            Self::Create { .. } => None,
            Self::Patch { key, .. }
            | Self::ExtendExpiry { key, .. }
            | Self::TransferOwnership { key, .. }
            | Self::Delete { key } => Some(key),
        }
    }

    pub fn expiry(&self) -> Option<&Expiry> {
        match self {
            Self::Create { expiry, .. } | Self::ExtendExpiry { expiry, .. } => Some(expiry),
            _ => None,
        }
    }

    pub fn validate(&self) -> Result<(), ArkivFault> {
        if self.key().is_some_and(|key| key.as_bytes().len() != 32) {
            return Err(ArkivFault::Value);
        }
        let mut names = std::collections::BTreeSet::new();
        let attrs = match self {
            Self::Create {
                flags, attributes, ..
            } => {
                if u64::from(*flags) & !ALL_FLAGS != 0 {
                    return Err(ArkivFault::Flags);
                }
                attributes.as_slice()
            }
            Self::Patch { set, .. } => set.as_slice(),
            _ => &[],
        };
        if attrs.len() > MAX_ATTRIBUTES {
            return Err(ArkivFault::Value);
        }
        if let Self::Patch { remove, .. } = self
            && attrs.len() + remove.len() > MAX_ATTRIBUTES
        {
            return Err(ArkivFault::Value);
        }
        for (name, cell) in attrs {
            if !names.insert(name) {
                return Err(ArkivFault::Duplicate);
            }
            validate_user_cell(name, cell)?;
        }
        // A create's values are the whole entity; a patch's set is part of it.
        if entity_bytes(attrs) > MAX_ENTITY_BYTES {
            return Err(ArkivFault::EntityTooLarge);
        }
        if let Self::Patch { remove, .. } = self {
            for name in remove {
                if !names.insert(name) {
                    return Err(ArkivFault::Duplicate);
                }
                validate_remove(name)?;
            }
        }
        Ok(())
    }
}

/// The byte each encoded [`ArkivOperation`] starts with.
mod tag {
    pub const CREATE: u8 = 1;
    pub const PATCH: u8 = 2;
    pub const EXTEND_EXPIRY: u8 = 3;
    pub const TRANSFER_OWNERSHIP: u8 = 4;
    pub const DELETE: u8 = 5;
}

impl Encode for ArkivOperation {
    fn encode_to(&self, encoder: &mut Encoder) {
        match self {
            Self::Create {
                salt,
                flags,
                expiry,
                attributes,
            } => {
                encoder.u8(tag::CREATE);
                encoder.fixed(&salt.to_be_bytes());
                encoder.u8(*flags);
                encoder.put(expiry);
                encoder.put(attributes)
            }
            Self::Patch { key, set, remove } => {
                encoder.u8(tag::PATCH);
                encoder.put(key);
                encoder.put(set);
                encoder.put(remove)
            }
            Self::ExtendExpiry { key, expiry } => {
                encoder.u8(tag::EXTEND_EXPIRY);
                encoder.put(key);
                encoder.put(expiry)
            }
            Self::TransferOwnership { key, owner } => {
                encoder.u8(tag::TRANSFER_OWNERSHIP);
                encoder.put(key);
                encoder.fixed(owner)
            }
            Self::Delete { key } => {
                encoder.u8(tag::DELETE);
                encoder.put(key)
            }
        }
    }
}

impl Decode for ArkivOperation {
    const MIN_ENCODED_LEN: usize = 1;
    fn decode_from(decoder: &mut Decoder<'_>) -> Result<Self, DecodeError> {
        Ok(match decoder.u8()? {
            tag::CREATE => Self::Create {
                salt: u128::from_be_bytes(decoder.array()?),
                flags: decoder.u8()?,
                expiry: decoder.get()?,
                attributes: decoder.get()?,
            },
            tag::PATCH => Self::Patch {
                key: decoder.get()?,
                set: decoder.get()?,
                remove: decoder.get()?,
            },
            tag::EXTEND_EXPIRY => Self::ExtendExpiry {
                key: decoder.get()?,
                expiry: decoder.get()?,
            },
            tag::TRANSFER_OWNERSHIP => Self::TransferOwnership {
                key: decoder.get()?,
                owner: decoder.array()?,
            },
            tag::DELETE => Self::Delete {
                key: decoder.get()?,
            },
            tag => {
                return Err(DecodeError::UnknownTag {
                    kind: "ArkivOperation",
                    tag,
                });
            }
        })
    }
}

/// Creation flag: the entity can never be patched.
pub const FLAG_READONLY: u8 = 1;
/// Creation flag: anyone may extend the entity's expiry, up to one rent cap
/// ahead.
pub const FLAG_PERMISSIONLESS_EXTENSION: u8 = 2;

pub const ARKIV_ADDRESS: [u8; 20] = [
    0x44, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0x44,
];
pub fn entity_namespace() -> NamespaceId {
    NamespaceId::from_bytes(1u64.to_be_bytes()).expect("namespace one")
}

pub fn nonce_key(caller: &[u8; 20]) -> HostKey {
    let mut key = [0u8; 32];
    key[..12].copy_from_slice(b"arkiv/nonce/");
    key[12..].copy_from_slice(caller);
    key
}

pub fn derive_entity_key(chain_id: u64, caller: &[u8; 20], nonce: u64, salt: u128) -> [u8; 32] {
    let mut bytes = Vec::with_capacity(96);
    bytes.extend_from_slice(&[0; 24]);
    bytes.extend_from_slice(&chain_id.to_be_bytes());
    bytes.extend_from_slice(&ARKIV_ADDRESS);
    bytes.extend_from_slice(caller);
    bytes.extend_from_slice(&nonce.to_be_bytes());
    bytes.extend_from_slice(&salt.to_be_bytes());
    alloy_primitives::keccak256(bytes).0
}

#[derive(Clone, Copy, Debug)]
pub struct ArkivExecutionContext<'a> {
    pub caller: [u8; 20],
    pub block_height: u64,
    pub chain_id: u64,
    /// The chain's rent clock ([`crate::gas::RentClock::of`] its params):
    /// rent is never charged past `block_height + cap_blocks`.
    pub rent: crate::gas::RentClock,
    pub expiry_per_block: u32,
    /// How many heights past a full expiry height an operation may spill to
    /// ([`crate::exec::EXPIRY_SPILL_WINDOW`] in live blocks). Zero is strict:
    /// height-zero construction refuses a row whose height is full.
    pub expiry_spill_window: u64,
    /// The accounts that pay rent: the caller's balance is debited and the
    /// rent burned. `None` quotes rent without debiting it (estimation,
    /// chains without accounts and height-zero construction).
    pub payer: Option<&'a crate::accounts::AccountConfig>,
    /// Waives the rent of a `Create` with the permanent expiry (`u64::MAX`).
    /// Only height-zero construction of a permanent-prefill launch sets it
    /// (its permanent rows are explicitly sponsored); live blocks pass
    /// `false`, so a live permanent create pays the capped rent like any
    /// lifetime at or beyond the cap.
    pub allow_permanent_create: bool,
}

/// One executed operation, the rent it charged and the storage units it
/// added: both nonzero only when the operation applied (a payer was debited
/// the rent, or would have been).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ArkivOutcome {
    pub outcome: OpOutcome,
    pub rent_wei: alloy_primitives::U256,
    /// Storage units the operation added: a create's whole entity, a patch's
    /// positive growth, zero otherwise. Counted against
    /// [`crate::ChainParams::max_new_storage_units_per_block`].
    pub new_units: u64,
}

impl From<OpOutcome> for ArkivOutcome {
    fn from(outcome: OpOutcome) -> Self {
        Self {
            outcome,
            rent_wei: alloy_primitives::U256::ZERO,
            new_units: 0,
        }
    }
}

fn name(s: &str) -> CellName {
    CellName::new(s).expect("system name")
}

fn corrupt() -> ExecError {
    ExecError::Storage(arkiv_db::StorageError::new(
        arkiv_db::StorageErrorKind::Corrupt,
        "Arkiv committed state is malformed",
    ))
}

fn count(branch: &Branch, key: &HostKey) -> Result<u64, ExecError> {
    match branch.try_host_get(key)? {
        None => Ok(0),
        Some(stored) => Ok(u64::from_be_bytes(
            stored.as_slice().try_into().map_err(|_| corrupt())?,
        )),
    }
}

fn put_count(branch: &mut Branch, key: HostKey, value: u64) -> Result<(), ExecError> {
    if value == 0 {
        branch.try_host_remove(&key)?;
    } else {
        branch
            .try_host_put(key, &value.to_be_bytes())?
            .map_err(|_| corrupt())?;
    }
    Ok(())
}

/// The expiry height `asked` resolves to at the context's block. Entities
/// have no longest lifetime: the rent cap bounds what one costs.
pub(crate) fn expiry(
    asked: &Expiry,
    context: &ArkivExecutionContext<'_>,
) -> Result<u64, HostFault> {
    crate::exec::resolve(asked, context.block_height, u64::MAX)
        .map(Lifetime::expires_at)
        .map_err(HostFault::ExpiryInvalid)
}

/// Canonical creation cells shared by live execution and bulk genesis: the
/// [`SYSTEM_CELLS`] host cells, then the user attributes as given. Callers
/// validate the operation and expiry before invoking this helper.
pub(crate) fn create_cells(
    key: [u8; 32],
    creator: [u8; 20],
    height: u64,
    expires: u64,
    flags: u8,
    attributes: &[(CellName, Cell)],
) -> Vec<(CellName, Cell)> {
    let mut cells = vec![
        (name("$key"), Cell::attribute(Value::Key(key))),
        (name("$creator"), Cell::attribute(Value::Addr(creator))),
        (name("$owner"), Cell::attribute(Value::Addr(creator))),
        (name("$createdAt"), Cell::attribute(Value::U64(height))),
        (name("$updatedAt"), Cell::attribute(Value::U64(height))),
        (name("$expiresAt"), Cell::attribute(Value::U64(expires))),
        (
            name("$creationFlags"),
            Cell::field(Value::U64(u64::from(flags))),
        ),
    ];
    debug_assert!(
        attributes
            .iter()
            .all(|(n, context)| validate_user_cell(n, context).is_ok())
    );
    cells.extend_from_slice(attributes);
    cells
}

/// Why an operation stops before it applies.
enum Stop {
    /// The host refuses the operation; nothing of it applied.
    Refused(HostFault),
    /// Engine and storage errors stay fatal: they are never downgraded to a
    /// refusal.
    Fatal(ExecError),
}

impl From<ArkivFault> for Stop {
    fn from(fault: ArkivFault) -> Self {
        Self::Refused(HostFault::Arkiv(fault))
    }
}

impl From<HostFault> for Stop {
    fn from(fault: HostFault) -> Self {
        Self::Refused(fault)
    }
}

impl From<ExecError> for Stop {
    fn from(error: ExecError) -> Self {
        Self::Fatal(error)
    }
}

impl From<arkiv_db::StorageError> for Stop {
    fn from(error: arkiv_db::StorageError) -> Self {
        Self::Fatal(error.into())
    }
}

/// An entity as an operation finds it.
struct Existing {
    entity: ArkivEntityProjection,
    cells: Vec<(CellName, Cell)>,
}

/// The live entity `op` names, which the caller may apply `op` to.
fn authorized_entity(
    branch: &Branch,
    namespace: NamespaceId,
    context: &ArkivExecutionContext<'_>,
    op: &ArkivOperation,
    key: &RecordKey,
) -> Result<Existing, Stop> {
    let row = match branch.try_get_record(namespace, key, &Projection::ALL, Budget::unlimited())? {
        Ok(read) => read.value,
        Err(rejected) if rejected.reason == arkiv_db::Rejection::UnknownNamespace => None,
        Err(_) => return Err(corrupt().into()),
    };
    let row = row.ok_or(ArkivFault::NotFound)?;
    let entity = project(&row).map_err(|_| corrupt())?;
    if entity.expires_at <= context.block_height {
        return Err(ArkivFault::Expired.into());
    }
    let anyone_may = matches!(op, ArkivOperation::ExtendExpiry { .. })
        && entity.creation_flags & FLAG_PERMISSIONLESS_EXTENSION != 0;
    if entity.owner != context.caller && !anyone_may {
        return Err(ArkivFault::NotOwner.into());
    }
    Ok(Existing {
        entity,
        cells: row.cells,
    })
}

/// The expiry heights of an operation that sets one.
struct AdmittedExpiry {
    /// The height asked for. Rent is charged up to it.
    asked: u64,
    /// The height the entity is scheduled at: the first with room from
    /// `asked` on, so up to `expiry_spill_window` blocks later, which are
    /// free.
    scheduled: u64,
}

/// Admits the expiry `asked` for a new entity or for `existing`.
fn admit_expiry(
    branch: &Branch,
    context: &ArkivExecutionContext<'_>,
    asked: &Expiry,
    existing: Option<&ArkivEntityProjection>,
) -> Result<AdmittedExpiry, Stop> {
    let target = expiry(asked, context)?;
    // Permissionless extension admits anyone's extension up to one rent cap
    // ahead. Further, permanence included, is the owner's alone: rent stops
    // at the cap, so a longer extension would be a lifetime nobody paid for
    // beyond it.
    if existing.is_some_and(|entity| entity.owner != context.caller)
        && target
            > context
                .block_height
                .saturating_add(context.rent.cap_blocks())
    {
        return Err(ArkivFault::PermanenceOwnerOnly.into());
    }
    let old = existing.map(|entity| entity.expires_at);
    if old.is_some_and(|old| target < old) {
        return Err(HostFault::ExpiryShortened.into());
    }
    // Permanent entities are not scheduled, and an entity keeping its
    // height keeps its place; any other takes the first height with room.
    let scheduled = if target == u64::MAX || old == Some(target) {
        target
    } else {
        crate::exec::spill_expiry(
            target,
            context.expiry_spill_window,
            u64::MAX,
            context.expiry_per_block,
            |height| crate::exec::read_count(branch, &crate::exec::total_key(height)),
        )?
        .ok_or(HostFault::ExpiryCapacityFull)?
    };
    Ok(AdmittedExpiry {
        asked: target,
        scheduled,
    })
}

/// An admitted operation: what it writes and what it leaves of its entity.
struct Change {
    write: WriteOp,
    /// The entity before; `None` for a create.
    before: Option<Existing>,
    /// The expiry height after; `None` once deleted.
    expires_at: Option<u64>,
    /// The expiry rent is charged to: the one asked for, before any spill.
    paid_until: Option<u64>,
    /// The caller's creation nonce after a create.
    next_nonce: Option<u64>,
}

impl Change {
    /// A patch of `before` that also stamps `$updatedAt`.
    fn patch(
        namespace: NamespaceId,
        key: &RecordKey,
        context: &ArkivExecutionContext<'_>,
        before: Existing,
        set: Vec<(CellName, Cell)>,
        remove: Vec<CellName>,
        expiry: Option<AdmittedExpiry>,
    ) -> Self {
        let mut stamped = vec![(
            name("$updatedAt"),
            Cell::attribute(Value::U64(context.block_height)),
        )];
        stamped.extend(set);
        let old = before.entity.expires_at;
        Self {
            write: WriteOp::PatchRecord {
                namespace,
                key: key.clone(),
                set: stamped,
                remove,
            },
            before: Some(before),
            expires_at: Some(expiry.as_ref().map_or(old, |expiry| expiry.scheduled)),
            paid_until: Some(expiry.map_or(old, |expiry| expiry.asked)),
            next_nonce: None,
        }
    }
}

/// Checks `op` against the entity it names and the caller's authority.
fn admit(
    branch: &Branch,
    namespace: NamespaceId,
    context: &ArkivExecutionContext<'_>,
    op: &ArkivOperation,
) -> Result<Change, Stop> {
    let existing = |key| authorized_entity(branch, namespace, context, op, key);
    Ok(match op {
        ArkivOperation::Create {
            salt,
            flags,
            expiry,
            attributes,
        } => {
            let expiry = admit_expiry(branch, context, expiry, None)?;
            let nonce = count(branch, &nonce_key(&context.caller))?;
            let next_nonce = nonce.checked_add(1).ok_or(ArkivFault::NonceExhausted)?;
            let key = derive_entity_key(context.chain_id, &context.caller, nonce, *salt);
            Change {
                write: WriteOp::CreateRecord {
                    namespace,
                    key: RecordKey::new(&key).expect("32-byte key"),
                    cells: create_cells(
                        key,
                        context.caller,
                        context.block_height,
                        expiry.scheduled,
                        *flags,
                        attributes,
                    ),
                },
                before: None,
                expires_at: Some(expiry.scheduled),
                paid_until: Some(expiry.asked),
                next_nonce: Some(next_nonce),
            }
        }
        ArkivOperation::Patch { key, set, remove } => {
            let before = existing(key)?;
            if before.entity.creation_flags & FLAG_READONLY != 0 {
                return Err(ArkivFault::Readonly.into());
            }
            Change::patch(
                namespace,
                key,
                context,
                before,
                set.clone(),
                remove.clone(),
                None,
            )
        }
        ArkivOperation::ExtendExpiry { key, expiry } => {
            let before = existing(key)?;
            let expiry = admit_expiry(branch, context, expiry, Some(&before.entity))?;
            let set = vec![(
                name("$expiresAt"),
                Cell::attribute(Value::U64(expiry.scheduled)),
            )];
            Change::patch(namespace, key, context, before, set, vec![], Some(expiry))
        }
        ArkivOperation::TransferOwnership { key, owner } => {
            let before = existing(key)?;
            if *owner == [0; 20] || *owner == before.entity.owner {
                return Err(ArkivFault::Owner.into());
            }
            let set = vec![(name("$owner"), Cell::attribute(Value::Addr(*owner)))];
            Change::patch(namespace, key, context, before, set, vec![], None)
        }
        ArkivOperation::Delete { key } => Change {
            write: WriteOp::DeleteRecord {
                namespace,
                key: key.clone(),
            },
            before: Some(existing(key)?),
            expires_at: None,
            paid_until: None,
            next_nonce: None,
        },
    })
}

/// The rent `change` owes and the storage units it adds.
fn price(
    branch: &Branch,
    context: &ArkivExecutionContext<'_>,
    change: &Change,
) -> Result<(alloy_primitives::U256, u64), Stop> {
    let before_cells = change.before.as_ref().map(|before| before.cells.as_slice());
    // The entity as the operation leaves it; `None` for a delete.
    let after_cells = match &change.write {
        WriteOp::CreateRecord { cells, .. } => Some(cells.clone()),
        WriteOp::PatchRecord { set, remove, .. } => {
            Some(patched(before_cells.unwrap_or_default(), set, remove))
        }
        _ => None,
    };
    if entity_bytes(after_cells.as_deref().unwrap_or_default()) > MAX_ENTITY_BYTES {
        return Err(ArkivFault::EntityTooLarge.into());
    }
    let units = |cells: Option<&[(CellName, Cell)]>| match cells {
        Some(cells) => crate::gas::storage_units(cells).map(|units| units.total),
        None => Ok(0),
    };
    let (Ok(before), Ok(after)) = (units(before_cells), units(after_cells.as_deref())) else {
        return Err(ArkivFault::GasOverflow.into());
    };
    // Genesis permanent prefill: no time-based rent for a creation.
    let waived = context.allow_permanent_create
        && change.before.is_none()
        && change.expires_at == Some(u64::MAX);
    let rent = if crate::gas::is_active(branch.pricing()) && !waived {
        let old = change
            .before
            .as_ref()
            .map(|before| before.entity.expires_at);
        crate::gas::rent(
            before,
            after,
            old,
            change.paid_until,
            context.block_height,
            context.rent,
        )
        .map_err(|_| ArkivFault::GasOverflow)?
        .wei
    } else {
        0
    };
    Ok((
        alloy_primitives::U256::from(rent),
        after.saturating_sub(before),
    ))
}

/// Execute one operation; the enclosing transaction checkpoint provides atomic batches.
/// Engine/storage errors remain fatal and may never be downgraded to user rejections.
///
/// Under GLM pricing the operation's rent ([`crate::gas::rent`]) is quoted
/// before the engine write. It is not gas: the engine keeps the whole
/// `budget`. With a payer the caller must hold the rent after its
/// transaction's gas reservation, or the operation is refused with
/// [`ArkivFault::InsufficientRentBalance`] before anything is written;
/// after a successful write the rent is debited and burned.
pub fn execute_operation(
    branch: &mut Branch,
    namespace: NamespaceId,
    context: &ArkivExecutionContext<'_>,
    op: &ArkivOperation,
    budget: Budget,
) -> Result<ArkivOutcome, ExecError> {
    match apply(branch, namespace, context, op, budget) {
        Ok(applied) => Ok(applied),
        Err(Stop::Refused(fault)) => Ok(OpOutcome::HostRejected { fault }.into()),
        Err(Stop::Fatal(error)) => Err(error),
    }
}

fn apply(
    branch: &mut Branch,
    namespace: NamespaceId,
    context: &ArkivExecutionContext<'_>,
    op: &ArkivOperation,
    budget: Budget,
) -> Result<ArkivOutcome, Stop> {
    // GLM disables generic record mutations in every namespace, so all of
    // them can safely use Arkiv ownership and rent. Chains without GLM
    // pricing allow generic writes outside namespace one, so entities stay
    // confined to it there.
    if namespace != entity_namespace() && !crate::gas::is_active(branch.pricing()) {
        return Err(ArkivFault::Namespace.into());
    }
    op.validate()?;
    let change = admit(branch, namespace, context, op)?;
    let (rent, new_units) = price(branch, context, &change)?;
    let caller = arkiv_db::OwnerId::new(context.caller);
    let payer = context.payer.filter(|_| !rent.is_zero());
    if let Some(accounts) = payer
        && accounts.read(branch, caller)?.balance < rent
    {
        return Err(ArkivFault::InsufficientRentBalance.into());
    }
    let written = match branch.try_execute_host(change.write, budget)? {
        Ok(written) => written,
        Err(rejected) => {
            return Ok(OpOutcome::Rejected {
                reason: rejected.reason,
                receipt: rejected.receipt,
            }
            .into());
        }
    };
    if let Some(accounts) = payer {
        accounts.burn_from(branch, caller, rent)?;
    }
    if let Some(nonce) = change.next_nonce {
        put_count(branch, nonce_key(&context.caller), nonce)?;
    }
    crate::exec::reschedule(
        branch,
        namespace,
        change
            .before
            .map(|before| Lifetime::expiring_at(before.entity.expires_at)),
        change.expires_at.map(Lifetime::expiring_at),
    )?;
    Ok(ArkivOutcome {
        outcome: OpOutcome::Applied {
            effect: written.effect,
            receipt: written.receipt,
        },
        rent_wei: rent,
        new_units,
    })
}

/// `cells` after a patch that sets `set` and removes `remove`.
fn patched(
    cells: &[(CellName, Cell)],
    set: &[(CellName, Cell)],
    remove: &[CellName],
) -> Vec<(CellName, Cell)> {
    let mut cells = cells.to_vec();
    cells.retain(|(name, _)| !remove.contains(name) && !set.iter().any(|(n, _)| n == name));
    cells.extend(set.iter().cloned());
    cells
}

/// What the Arkiv operations of one transaction would charge, applied in
/// order on `branch` at `context.block_height`, each seeing what the earlier ones
/// created or changed: an admission and block-building estimate, never
/// consensus ([`execute_operation`] prices every operation again).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TxQuote {
    /// The rent: per operation the larger of its rent at `context.block_height` and
    /// `margin` blocks later. Rent is monotone in the height for every
    /// operation (an extension across the cap grows by one block of its size
    /// per block, everything else shrinks), so this covers any execution
    /// within `margin` blocks.
    pub rent_wei: alloy_primitives::U256,
    /// The storage units the operations add: a create's entity, a patch's
    /// positive growth. Exact when every operation applies to the state
    /// quoted; a transaction that fails adds nothing, so it never undercounts.
    pub new_units: u64,
}

/// The entity state the earlier operations of a quoted transaction left:
/// `None` once deleted.
type Overlay =
    std::collections::BTreeMap<(NamespaceId, Vec<u8>), Option<(Vec<(CellName, Cell)>, u64)>>;
/// [`TxQuote`] of `ops`. Reads at most each named entity once and the
/// caller's creation nonce; checks neither authority nor capacity, and an
/// operation that could not apply quotes nothing.
pub fn quote_tx<'o>(
    branch: &Branch,
    context: &ArkivExecutionContext<'_>,
    margin: u64,
    ops: impl IntoIterator<Item = (NamespaceId, &'o ArkivOperation)>,
) -> Result<TxQuote, ExecError> {
    let mut quote = TxQuote::default();
    let mut overlay = Overlay::new();
    let mut nonce = None;
    let units =
        |cells: &[(CellName, Cell)]| crate::gas::storage_units(cells).map(|units| units.total);
    let glm = crate::gas::is_active(branch.pricing());
    for (namespace, op) in ops {
        if op.validate().is_err() {
            continue;
        }
        let current = match op.key() {
            None => None,
            Some(key) => {
                let slot = (namespace, key.as_bytes().to_vec());
                let state = match overlay.get(&slot) {
                    Some(state) => state.clone(),
                    None => match branch.try_get_record(
                        namespace,
                        key,
                        &Projection::ALL,
                        Budget::unlimited(),
                    )? {
                        Ok(read) => read.value.and_then(|row| {
                            project(&row)
                                .ok()
                                .map(|entity| (row.cells, entity.expires_at))
                        }),
                        Err(_) => None,
                    },
                };
                match state {
                    Some((cells, expiry)) if expiry > context.block_height => Some((cells, expiry)),
                    _ => continue,
                }
            }
        };
        let old = current.as_ref().map(|(_, expiry)| *expiry);
        let target = |at: u64| match op.expiry() {
            Some(exp) => expiry(
                exp,
                &ArkivExecutionContext {
                    block_height: at,
                    ..*context
                },
            )
            .ok()
            .filter(|target| old.is_none_or(|old| *target >= old)),
            None => old,
        };
        let Some(new) = target(context.block_height) else {
            continue;
        };
        let stamp = (
            name("$updatedAt"),
            Cell::attribute(Value::U64(context.block_height)),
        );
        let current_cells = current.map(|(cells, _)| cells).unwrap_or_default();
        let (key, after_cells) = match op {
            ArkivOperation::Create {
                salt,
                flags,
                attributes,
                ..
            } => {
                let next = match nonce {
                    Some(n) => n,
                    None => count(branch, &nonce_key(&context.caller))?,
                };
                nonce = next.checked_add(1);
                let key = derive_entity_key(context.chain_id, &context.caller, next, *salt);
                let cells = create_cells(
                    key,
                    context.caller,
                    context.block_height,
                    new,
                    *flags,
                    attributes,
                );
                (key.to_vec(), cells)
            }
            ArkivOperation::Patch { key, set, remove } => {
                let mut changes = vec![stamp];
                changes.extend_from_slice(set);
                (
                    key.as_bytes().to_vec(),
                    patched(&current_cells, &changes, remove),
                )
            }
            ArkivOperation::ExtendExpiry { key, .. } => (
                key.as_bytes().to_vec(),
                patched(
                    &current_cells,
                    &[
                        stamp,
                        (name("$expiresAt"), Cell::attribute(Value::U64(new))),
                    ],
                    &[],
                ),
            ),
            ArkivOperation::TransferOwnership { .. } => continue,
            ArkivOperation::Delete { key } => {
                overlay.insert((namespace, key.as_bytes().to_vec()), None);
                continue;
            }
        };
        let (Ok(before), Ok(after)) = (
            if old.is_some() {
                units(&current_cells)
            } else {
                Ok(0)
            },
            units(&after_cells),
        ) else {
            continue;
        };
        quote.new_units = quote.new_units.saturating_add(after.saturating_sub(before));
        if glm {
            let rent_at = |at: u64| {
                target(at)
                    .and_then(|new| {
                        crate::gas::rent(before, after, old, Some(new), at, context.rent).ok()
                    })
                    .map_or(0, |quote| quote.wei)
            };
            let rent = rent_at(context.block_height)
                .max(rent_at(context.block_height.saturating_add(margin)));
            quote.rent_wei = quote
                .rent_wei
                .saturating_add(alloy_primitives::U256::from(rent));
        }
        overlay.insert((namespace, key), Some((after_cells, new)));
    }
    Ok(quote)
}

#[cfg(test)]
mod tests;
