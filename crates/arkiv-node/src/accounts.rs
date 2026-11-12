//! Fixed-price native GLM accounts. Absent records resolve to immutable genesis allocations.
//! The signed genesis hash authenticates those defaults; host-trie records override them.

use crate::{HostFault, OpOutcome, TxEnvelope, TxOp};
use alloy_primitives::U256;
use arkiv_db::{
    Branch, Budget, Decode, DecodeError, Decoder, Encode, Encoder, OwnerId, StorageError,
    StorageErrorKind,
};

pub const GAS_PRICE_WEI: u64 = 1_000_000_000;
/// Additional work for transfers inside multi-operation transactions.
pub const TRANSFER_GAS: u64 = 9_000;
pub const MAX_ALLOCATIONS: usize = 256;
pub const BURN_KEY: [u8; 32] = *b"arkiv/glm/burned/v1_____________";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AccountFault {
    InsufficientBalance,
    Overflow,
    Unsupported,
}

impl Encode for AccountFault {
    fn encode_to(&self, encoder: &mut Encoder) {
        encoder.u8(match self {
            Self::InsufficientBalance => 1,
            Self::Overflow => 2,
            Self::Unsupported => 3,
        });
    }
}

impl Decode for AccountFault {
    fn decode_from(decoder: &mut Decoder<'_>) -> Result<Self, DecodeError> {
        match decoder.u8()? {
            1 => Ok(Self::InsufficientBalance),
            2 => Ok(Self::Overflow),
            3 => Ok(Self::Unsupported),
            tag => Err(DecodeError::UnknownTag {
                kind: "AccountFault",
                tag,
            }),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AccountConfig {
    pub gas_price_wei: u64,
    pub allocations: Vec<(OwnerId, U256)>,
}

impl AccountConfig {
    pub fn new(mut allocations: Vec<(OwnerId, U256)>) -> Result<Self, AccountFault> {
        allocations.sort_by_key(|(actor, _)| *actor);
        let value = Self {
            gas_price_wei: GAS_PRICE_WEI,
            allocations,
        };
        value.validate()?;
        Ok(value)
    }

    pub fn validate(&self) -> Result<(), AccountFault> {
        if self.gas_price_wei != GAS_PRICE_WEI
            || self.allocations.len() > MAX_ALLOCATIONS
            || self
                .allocations
                .windows(2)
                .any(|pair| pair[0].0 >= pair[1].0)
            || self
                .allocations
                .iter()
                .any(|(_, balance)| balance.is_zero())
        {
            return Err(AccountFault::Unsupported);
        }
        self.initial_supply()?;
        Ok(())
    }

    pub fn initial_supply(&self) -> Result<U256, AccountFault> {
        self.allocations
            .iter()
            .try_fold(U256::ZERO, |sum, (_, balance)| {
                sum.checked_add(*balance).ok_or(AccountFault::Overflow)
            })
    }

    pub fn initial_balance(&self, actor: OwnerId) -> U256 {
        self.allocations
            .binary_search_by_key(&actor, |(owner, _)| *owner)
            .map(|index| self.allocations[index].1)
            .unwrap_or(U256::ZERO)
    }

    pub fn resolve(&self, actor: OwnerId, value: Option<Vec<u8>>) -> Result<Account, StorageError> {
        match value {
            None => Ok(Account {
                nonce: 0,
                balance: self.initial_balance(actor),
            }),
            Some(stored) => arkiv_db::decode_exact(&stored).map_err(|_| corrupt()),
        }
    }

    pub fn read(&self, branch: &Branch, actor: OwnerId) -> Result<Account, StorageError> {
        self.resolve(actor, branch.try_host_get(&account_key(actor))?)
    }

    pub fn reserve(&self, branch: &mut Branch, tx: &TxEnvelope) -> Result<(), crate::ExecError> {
        self.debit(branch, tx.actor, fee(tx.tx_budget))
    }

    pub fn settle(
        &self,
        branch: &mut Branch,
        tx: &TxEnvelope,
        spent: u64,
    ) -> Result<(), crate::ExecError> {
        let refund = fee(tx
            .tx_budget
            .checked_sub(spent)
            .ok_or(crate::ExecError::Account(AccountFault::Overflow))?);
        let mut account = self.read(branch, tx.actor)?;
        account.balance = account
            .balance
            .checked_add(refund)
            .ok_or(crate::ExecError::Account(AccountFault::Overflow))?;
        put(branch, tx.actor, account)?;
        self.burn(branch, fee(spent))
    }

    /// Debits `amount` from `actor` and burns it: entity rent, which is not
    /// gas. The caller has checked the balance; a shortfall here is a broken
    /// invariant, never a user rejection.
    pub fn burn_from(
        &self,
        branch: &mut Branch,
        actor: OwnerId,
        amount: U256,
    ) -> Result<(), crate::ExecError> {
        self.debit(branch, actor, amount)?;
        self.burn(branch, amount)
    }

    fn debit(
        &self,
        branch: &mut Branch,
        actor: OwnerId,
        amount: U256,
    ) -> Result<(), crate::ExecError> {
        let mut account = self.read(branch, actor)?;
        account.balance = account
            .balance
            .checked_sub(amount)
            .ok_or(crate::ExecError::Account(AccountFault::InsufficientBalance))?;
        put(branch, actor, account)?;
        Ok(())
    }

    /// Adds `amount` to the burned total, which never exceeds the supply.
    fn burn(&self, branch: &mut Branch, amount: U256) -> Result<(), crate::ExecError> {
        let burnt = burned(branch.try_host_get(&BURN_KEY)?)?
            .checked_add(amount)
            .ok_or(crate::ExecError::Account(AccountFault::Overflow))?;
        if burnt > self.initial_supply().map_err(crate::ExecError::Account)? {
            return Err(crate::ExecError::Account(AccountFault::Overflow));
        }
        branch
            .try_host_put(BURN_KEY, &burnt.to_be_bytes::<32>())?
            .map_err(|_| corrupt())?;
        Ok(())
    }

    pub fn transfer(
        &self,
        branch: &mut Branch,
        actor: OwnerId,
        to: OwnerId,
        amount: U256,
        remaining: u64,
        intrinsic_covers_transfer: bool,
    ) -> Result<OpOutcome, StorageError> {
        // Standalone transfers are covered by transaction intrinsic gas. Batches
        // still meter each transfer, including failed balance checks.
        let gas = if intrinsic_covers_transfer {
            0
        } else {
            TRANSFER_GAS
        };
        let receipt = match branch.try_charge_host(gas, Budget::limit(remaining))? {
            Ok(receipt) => receipt,
            Err(rejected) => {
                return Ok(OpOutcome::Rejected {
                    reason: rejected.reason,
                    receipt: rejected.receipt,
                });
            }
        };
        let mut sender = self.read(branch, actor)?;
        let Some(debit) = sender.balance.checked_sub(amount) else {
            return Ok(OpOutcome::AccountRejected {
                fault: AccountFault::InsufficientBalance,
                receipt,
            });
        };
        if actor != to && !amount.is_zero() {
            let mut receiver = self.read(branch, to)?;
            let Some(credit) = receiver.balance.checked_add(amount) else {
                return Ok(OpOutcome::AccountRejected {
                    fault: AccountFault::Overflow,
                    receipt,
                });
            };
            sender.balance = debit;
            receiver.balance = credit;
            put(branch, actor, sender)?;
            put(branch, to, receiver)?;
        }
        Ok(OpOutcome::Applied {
            effect: arkiv_db::Effect::BalanceTransferred {
                to,
                amount: amount.to_be_bytes::<32>(),
            },
            receipt,
        })
    }
}

impl Encode for AccountConfig {
    fn encode_to(&self, encoder: &mut Encoder) {
        encoder.u64(self.gas_price_wei);
        encoder.u32(self.allocations.len() as u32);
        for (owner, balance) in &self.allocations {
            encoder.put(owner);
            encoder.fixed(&balance.to_be_bytes::<32>());
        }
    }
}

impl Decode for AccountConfig {
    const MIN_ENCODED_LEN: usize = 12;
    fn decode_from(decoder: &mut Decoder<'_>) -> Result<Self, DecodeError> {
        /// A 20-byte owner and a 32-byte balance.
        const ALLOCATION_LEN: usize = 52;
        let gas_price_wei = decoder.u64()?;
        let count = decoder.u32()? as usize;
        if count > MAX_ALLOCATIONS || count > decoder.remaining() / ALLOCATION_LEN {
            return Err(DecodeError::Invalid {
                kind: "AccountConfig",
                reason: "allocation count",
            });
        }
        let mut allocations = Vec::with_capacity(count);
        for _ in 0..count {
            allocations.push((decoder.get()?, U256::from_be_bytes(decoder.array::<32>()?)));
        }
        let config = Self {
            gas_price_wei,
            allocations,
        };
        config.validate().map_err(|_| DecodeError::Invalid {
            kind: "AccountConfig",
            reason: "invalid allocations or price",
        })?;
        Ok(config)
    }
}

/// The version tag a stored [`Account`] starts with.
const ACCOUNT_VERSION: u32 = 1;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Account {
    pub nonce: u64,
    pub balance: U256,
}

impl Encode for Account {
    fn encode_to(&self, encoder: &mut Encoder) {
        encoder.u32(ACCOUNT_VERSION);
        encoder.u64(self.nonce);
        encoder.fixed(&self.balance.to_be_bytes::<32>());
    }
}

impl Decode for Account {
    const MIN_ENCODED_LEN: usize = 44;
    fn decode_from(decoder: &mut Decoder<'_>) -> Result<Self, DecodeError> {
        if decoder.u32()? != ACCOUNT_VERSION {
            return Err(DecodeError::Invalid {
                kind: "Account",
                reason: "version",
            });
        }
        Ok(Self {
            nonce: decoder.u64()?,
            balance: U256::from_be_bytes(decoder.array::<32>()?),
        })
    }
}

pub fn account_key(actor: OwnerId) -> [u8; 32] {
    let mut key = [0; 32];
    key[..12].copy_from_slice(b"arkiv/glm/a/");
    key[12..].copy_from_slice(actor.as_bytes());
    key
}

pub fn put(branch: &mut Branch, actor: OwnerId, account: Account) -> Result<(), StorageError> {
    branch
        .try_host_put(account_key(actor), &arkiv_db::encode(&account))?
        .map_err(|_| corrupt())?;
    Ok(())
}

pub fn burned(bytes: Option<Vec<u8>>) -> Result<U256, StorageError> {
    match bytes {
        None => Ok(U256::ZERO),
        Some(stored) => Ok(U256::from_be_bytes(
            <[u8; 32]>::try_from(stored).map_err(|_| corrupt())?,
        )),
    }
}

pub fn fee(gas: u64) -> U256 {
    U256::from(gas) * U256::from(GAS_PRICE_WEI)
}

/// Conservative mempool reservation: fees plus gross non-self outgoing value.
/// The pool adds the rent its admission simulation quoted
/// ([`crate::signed::pool::Pool::insert_funded`]); execution checks rent
/// again against the balance left after the gas reservation.
pub fn reservation(tx: &TxEnvelope) -> Result<U256, AccountFault> {
    tx.ops
        .iter()
        .try_fold(fee(tx.tx_budget), |sum, op| match op {
            TxOp::Transfer { to, amount } if *to != tx.actor => {
                sum.checked_add(*amount).ok_or(AccountFault::Overflow)
            }
            _ => Ok(sum),
        })
}

fn corrupt() -> StorageError {
    StorageError::new(StorageErrorKind::Corrupt, "account state")
}

pub fn unsupported() -> OpOutcome {
    OpOutcome::HostRejected {
        fault: HostFault::Account(AccountFault::Unsupported),
    }
}
