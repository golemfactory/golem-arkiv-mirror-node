use std::fmt;

use crate::{
    meter::{PhysicalTrace, Receipt, Rejection},
    namespace::{NamespaceId, NamespaceName, OwnerId, Revision},
    record::{Cell, CellName, Limits, RecordId, RecordKey},
    typed,
};

/// A host-authorized write. The host validates the request and its signature
/// and passes the authenticated actor separately.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WriteOp {
    /// Allocate a namespace owned by the actor. Creation is permissionless
    /// and gated only by its charge and the directory capacity.
    CreateNamespace {
        /// Readable name; registration and uniqueness are host policy.
        name: NamespaceName,
    },
    /// Replace the owner of a namespace the actor currently owns.
    TransferNamespace {
        /// The namespace to transfer.
        namespace: NamespaceId,
        /// The revision the request was built against.
        expected_revision: Revision,
        /// The new owner; may equal the current owner.
        new_owner: OwnerId,
    },
    /// Insert or replace a record in a namespace the actor owns.
    Put {
        /// Target namespace.
        namespace: NamespaceId,
        /// Record key.
        key: Vec<u8>,
        /// Record value; empty values are valid.
        value: Vec<u8>,
    },
    /// Remove a record from a namespace the actor owns; missing keys succeed.
    Delete {
        /// Target namespace.
        namespace: NamespaceId,
        /// Record key.
        key: Vec<u8>,
    },
    /// Create a typed record under a key not currently live in a namespace
    /// the actor owns; the record's attribute cells are indexed at once.
    CreateRecord {
        /// Target namespace.
        namespace: NamespaceId,
        /// Caller-assigned key; `AlreadyExists` if bound.
        key: RecordKey,
        /// Cells in any order; names must be unique.
        cells: Vec<(CellName, Cell)>,
    },
    /// Set and remove cells of a live typed record; kind and type may
    /// change and the index follows in the same operation.
    PatchRecord {
        /// Target namespace.
        namespace: NamespaceId,
        /// Key of the record; `NotFound` if not live.
        key: RecordKey,
        /// Cells to create or replace.
        set: Vec<(CellName, Cell)>,
        /// Names to remove; an absent name is a charged no-op.
        remove: Vec<CellName>,
    },
    /// Remove a live typed record and every index membership it holds.
    DeleteRecord {
        /// Target namespace.
        namespace: NamespaceId,
        /// Key of the record; `NotFound` if not live.
        key: RecordKey,
    },
}

impl WriteOp {
    /// Validates a write's arguments against the instance limits: exactly
    /// the argument checks of admission, which run before any charge and
    /// reject with [`Rejection::InvalidArgument`] without recording
    /// anything. They depend only on the operation and `limits`, never on
    /// state, so a host can apply them before accepting a request.
    pub fn validate(&self, limits: &Limits) -> Result<(), Rejection> {
        let outcome = match self {
            Self::Put { key, .. } | Self::Delete { key, .. }
                if key.len() > Limits::MAX_RAW_KEY_LEN =>
            {
                Err(crate::ArgumentFault::KeyTooLong)
            }
            Self::CreateRecord { cells, .. } => typed::validate_create(limits, cells),
            Self::PatchRecord { set, remove, .. } => typed::validate_patch(limits, set, remove),
            _ => Ok(()),
        };
        outcome.map_err(Rejection::InvalidArgument)
    }
}

/// What an applied write changed.
///
/// Effects are provisional until the candidate is published: a rolled-back
/// or discarded creation releases its ID, which a later creation may reuse.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Effect {
    /// A host-account transfer, represented in GLM base units.
    BalanceTransferred { to: OwnerId, amount: [u8; 32] },
    /// A namespace was allocated with this ID.
    NamespaceCreated(NamespaceId),
    /// The descriptor now carries this revision.
    Transferred {
        /// The new revision.
        revision: Revision,
    },
    /// A record was inserted (`replaced == false`) or replaced.
    Put {
        /// Whether the key previously existed.
        replaced: bool,
    },
    /// A key was removed, or was already absent.
    Deleted {
        /// Whether the key previously existed.
        existed: bool,
    },
    /// A typed record was created.
    RecordCreated {
        /// The minted id.
        id: RecordId,
    },
    /// A typed record was patched.
    RecordPatched {
        /// The record's id.
        id: RecordId,
    },
    /// A typed record was deleted.
    RecordDeleted {
        /// The id the record had.
        id: RecordId,
    },
}

/// Who authorizes a write: a host-authenticated principal checked against
/// the namespace owner, or the host itself for chain-driven transitions.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Authority {
    Owner(OwnerId),
    Host,
}

/// A successful write with its receipt.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Executed {
    /// The change applied.
    pub effect: Effect,
    /// The canonical cost record.
    pub receipt: Receipt,
    /// Non-canonical physical measurement of the work.
    pub physical: PhysicalTrace,
}

/// A rejected operation: no logical change, but the receipt is still issued.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Rejected {
    /// Why the operation stopped.
    pub reason: Rejection,
    /// The canonical cost record of the work charged before it stopped.
    pub receipt: Receipt,
    /// Non-canonical physical measurement of the work.
    pub physical: PhysicalTrace,
}

impl fmt::Display for Rejected {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{} (spent {})", self.reason, self.receipt.spent)
    }
}

impl std::error::Error for Rejected {}

/// A successful read with its receipt.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Metered<T> {
    /// The read result.
    pub value: T,
    /// The canonical read receipt; never a gas charge.
    pub receipt: Receipt,
    /// Non-canonical physical measurement of the work.
    pub physical: PhysicalTrace,
}

/// Outcome of a write.
pub type WriteResult = Result<Executed, Rejected>;

/// Outcome of a read.
pub type ReadResult<T> = Result<Metered<T>, Rejected>;
