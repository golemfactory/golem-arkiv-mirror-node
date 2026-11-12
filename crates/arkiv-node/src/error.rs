//! The error of the signed node and the simulator: a closed set of codes,
//! each rendered on the wire and on stderr as its own name.

use std::fmt;

pub type Result<T> = std::result::Result<T, Error>;

macro_rules! error_codes {
    ($($name:ident),* $(,)?) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub enum ErrorCode {
            $($name),*
        }

        impl ErrorCode {
            /// The code as clients and scripts see it.
            pub const fn as_str(self) -> &'static str {
                match self {
                    $(Self::$name => stringify!($name)),*
                }
            }

            /// The code a peer's error body names, if it is one of ours.
            pub fn parse(s: &str) -> Option<Self> {
                match s {
                    $(stringify!($name) => Some(Self::$name),)*
                    _ => None,
                }
            }
        }
    };
}

error_codes! {
    AccountOverflow,
    ArrivalExhausted,
    BalanceOverflow,
    BlockNotSynced,
    BlockTooLarge,
    BootstrapFailed,
    BudgetExceeded,
    CapacityReached,
    ChainConflict,
    ChainInitializing,
    CommitUncertain,
    CursorMismatch,
    ExceedsStorageCap,
    ExecutionReverted,
    FilterNotFound,
    FollowerOnly,
    FullNodeRequired,
    GasLimitBelowIntrinsic,
    GasPriceBelowMinimum,
    HistoryPruned,
    HistoryUnavailable,
    IdentityMismatch,
    InputsTooLarge,
    InsufficientBalance,
    InvalidAccountGenesis,
    InvalidAllocations,
    InvalidAmount,
    InvalidArkivAttribute,
    InvalidArkivCalldata,
    InvalidArkivOperation,
    InvalidBlock,
    InvalidBlockCommitment,
    InvalidBootstrap,
    InvalidEntity,
    InvalidEthereumEnvelope,
    InvalidEthereumTransaction,
    InvalidGasSchedule,
    InvalidGenesis,
    InvalidGenesisBlock,
    InvalidGenesisBundle,
    InvalidHeader,
    InvalidHeight,
    InvalidInputs,
    InvalidKey,
    InvalidNonce,
    InvalidParams,
    InvalidPoolLimits,
    InvalidPrefillBundle,
    InvalidPrefillGenesis,
    InvalidProof,
    InvalidRangeLimits,
    InvalidRequest,
    InvalidSignature,
    InvalidSignedGenesis,
    InvalidSigningKey,
    InvalidTransaction,
    LogRangeTooLarge,
    MethodNotFound,
    MetricsBindFailed,
    NamespaceNotFound,
    NonceConflict,
    NonceExhausted,
    NonceTooHigh,
    NonceTooLow,
    NonpayableCall,
    NonzeroArkivValue,
    NoPendingHistory,
    PeerUnavailable,
    PendingHistoryMismatch,
    PoolFull,
    PrefillBundleRequired,
    ProducerOnly,
    ProofLimitExceeded,
    QueueFull,
    ReadDeadlineExceeded,
    ReadRejected,
    ReadUnavailable,
    RequestConflict,
    ResourceLimit,
    ResponseTooLarge,
    RpcIndexCapacity,
    ServerBusy,
    SigningFailed,
    SimulationFailed,
    StorageCorrupt,
    StorageUnavailable,
    TimestampOverflow,
    TransactionDoesNotFit,
    TransactionDroppedExceedsStorageCap,
    TransactionDroppedInsufficientBalance,
    TransactionTooLarge,
    TreePathNotFound,
    Unauthorized,
    UnfundedBootstrap,
    UnknownBlock,
    UnknownRun,
    UnprotectedEthereumTransaction,
    UnsupportedArkivOperation,
    UnsupportedContractCall,
    UnsupportedContractCreation,
    UnsupportedEthereumCall,
    UnsupportedEthereumTransactionType,
    UnsupportedGasOperation,
    UnsupportedHistoricalSimulation,
    UnsupportedNodeRole,
    UnsupportedProof,
    UnsupportedQueryExecution,
    UnsupportedVersion,
}

impl fmt::Display for ErrorCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// What a client is told beside the code.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Detail {
    /// The height a `HistoryPruned` refusal was asked for and the floor it
    /// fell below.
    Pruned { requested: u64, floor: u64 },
    /// The revert data of an `ExecutionReverted` simulation.
    Revert(String),
}

#[derive(Clone)]
pub struct Error {
    pub code: ErrorCode,
    pub detail: Option<Detail>,
    /// What failed underneath, for the operator: written to stderr, never
    /// sent to a client, and not part of the error's identity.
    pub cause: Option<String>,
}

impl PartialEq for Error {
    fn eq(&self, other: &Self) -> bool {
        (self.code, &self.detail) == (other.code, &other.detail)
    }
}

impl Eq for Error {}

impl fmt::Debug for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut debug = f.debug_struct("Error");
        debug.field("code", &self.code);
        debug.field("detail", &self.detail);
        if let Some(cause) = &self.cause {
            debug.field("cause", cause);
        }
        debug.finish()
    }
}

impl ErrorCode {
    /// This code for a failure whose `cause` the operator should see.
    pub fn caused_by(self, cause: impl fmt::Display) -> Error {
        Error {
            cause: Some(cause.to_string()),
            ..self.into()
        }
    }
}

impl Error {
    pub fn pruned(requested: u64, floor: u64) -> Self {
        Self {
            detail: Some(Detail::Pruned { requested, floor }),
            ..ErrorCode::HistoryPruned.into()
        }
    }

    pub fn reverted(data: Option<String>) -> Self {
        Self {
            detail: data.map(Detail::Revert),
            ..ErrorCode::ExecutionReverted.into()
        }
    }

    /// Writes the cause of a storage failure to stderr: the client is told
    /// the code alone.
    pub(crate) fn log_storage_failure(&self) {
        if matches!(
            self.code,
            ErrorCode::StorageUnavailable | ErrorCode::StorageCorrupt
        ) && self.cause.is_some()
        {
            eprintln!("{}", self.diagnostic());
        }
    }

    /// The code and, when known, its cause: a line for the operator's log.
    pub fn diagnostic(&self) -> String {
        match &self.cause {
            Some(cause) => format!("{}: {cause}", self.code),
            None => self.code.to_string(),
        }
    }

    /// `(requested, floor)` of a `HistoryPruned` refusal that names them.
    pub fn pruned_at(&self) -> Option<(u64, u64)> {
        match self.detail {
            Some(Detail::Pruned { requested, floor }) => Some((requested, floor)),
            _ => None,
        }
    }
}

impl From<ErrorCode> for Error {
    fn from(code: ErrorCode) -> Self {
        Self {
            code,
            detail: None,
            cause: None,
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.code.as_str())
    }
}

impl std::error::Error for Error {}

impl From<arkiv_db::StorageError> for Error {
    fn from(e: arkiv_db::StorageError) -> Self {
        use arkiv_db::StorageErrorKind;
        match e.kind {
            StorageErrorKind::Corrupt | StorageErrorKind::Missing => ErrorCode::StorageCorrupt,
            StorageErrorKind::Incompatible => ErrorCode::IdentityMismatch,
            _ => ErrorCode::StorageUnavailable,
        }
        .caused_by(e)
    }
}

impl From<arkiv_db::durable::DurableError> for Error {
    fn from(e: arkiv_db::durable::DurableError) -> Self {
        use arkiv_db::durable::DurableError;
        match e {
            DurableError::Storage(e) => e.into(),
            DurableError::ResourceLimit(_) => ErrorCode::CapacityReached.caused_by(e),
            // Retention refusal: explicit, never storage corruption or absence.
            DurableError::Engine(arkiv_db::EngineError::HistoryPruned { requested, floor }) => {
                Self::pruned(requested.sequence(), floor.sequence())
            }
            _ => ErrorCode::StorageUnavailable.caused_by(e),
        }
    }
}

impl From<arkiv_db::proof::ProofError> for Error {
    fn from(e: arkiv_db::proof::ProofError) -> Self {
        use arkiv_db::proof::ProofError;
        match e {
            ProofError::UnsupportedVersion => ErrorCode::UnsupportedVersion,
            ProofError::UnsupportedProof => ErrorCode::UnsupportedProof,
            ProofError::InvalidRequest => ErrorCode::InvalidRequest,
            ProofError::CursorMismatch => ErrorCode::CursorMismatch,
            ProofError::ProofLimitExceeded => ErrorCode::ProofLimitExceeded,
            ProofError::BudgetExceeded => ErrorCode::BudgetExceeded,
            ProofError::StorageUnavailable => ErrorCode::StorageUnavailable,
            ProofError::InvalidProof => ErrorCode::InvalidProof,
        }
        .into()
    }
}
