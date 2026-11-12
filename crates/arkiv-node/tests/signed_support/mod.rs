//! Shared builders for the signed-chain tests: every signed chain runs the
//! `glm-v2` profile, so a test genesis funds each actor it uses.

#![allow(dead_code)]

pub mod http;

use alloy_primitives::U256;
use arkiv_db::{OwnerId, durable::Options};
use arkiv_db_memory::MemoryVersionedStore;
use arkiv_db_storage::versioned::VersionedStore;
use arkiv_node::{
    Genesis, TxEnvelope, TxOp,
    accounts::AccountConfig,
    arkiv::{self, ArkivOperation},
    signed::{
        crypto::Signer,
        history::History,
        host::{Host, Launch, Prefill},
        protocol::{SignedGenesis, SignedHeader, SignedTx, default_native_genesis},
    },
};
use std::sync::Arc;

/// The balance each funded test actor starts with: one million GLM.
pub fn balance() -> U256 {
    U256::from(10u64).pow(U256::from(24u64))
}

/// Allocations giving every actor in `funded` [`balance`].
pub fn allocations(funded: &[OwnerId]) -> AccountConfig {
    AccountConfig::new(funded.iter().map(|actor| (*actor, balance())).collect()).unwrap()
}

/// The deterministic test signer for `byte`.
pub fn signer(byte: u8) -> Signer {
    Signer::from_bytes([byte; 32]).unwrap()
}

/// The test signer whose private key is the number `n`.
pub fn scalar_signer(n: u16) -> Signer {
    let mut key = [0; 32];
    key[30..].copy_from_slice(&n.to_be_bytes());
    Signer::from_bytes(key).unwrap()
}

/// The addresses of the signers for `bytes`.
pub fn actors(bytes: &[u8]) -> Vec<OwnerId> {
    bytes.iter().map(|b| signer(*b).address()).collect()
}

/// A signed genesis over `native`, proposed by `proposer`, funding the
/// signers for `funded` (the proposer is always funded).
pub fn genesis_from(
    native: Genesis,
    run_id: [u8; 16],
    proposer: u8,
    block_period_ms: u64,
    funded: &[u8],
) -> SignedGenesis {
    let mut bytes = funded.to_vec();
    if !bytes.contains(&proposer) {
        bytes.push(proposer);
    }
    SignedGenesis::new(
        native,
        run_id,
        *signer(proposer).address().as_bytes(),
        block_period_ms,
        allocations(&actors(&bytes)),
    )
    .unwrap()
}

/// A signed genesis over the default native genesis.
pub fn genesis(
    chain_id: u64,
    timestamp_ms: u64,
    run_id: [u8; 16],
    proposer: u8,
    block_period_ms: u64,
    funded: &[u8],
) -> SignedGenesis {
    genesis_from(
        default_native_genesis(chain_id, timestamp_ms, block_period_ms),
        run_id,
        proposer,
        block_period_ms,
        funded,
    )
}

/// What launches a host of `genesis` as node `source`, its history in the
/// empty `history` store: the genesis certificate is `proposer`'s, who also
/// signs blocks when the host is a `producer`.
pub fn launch(
    genesis: &SignedGenesis,
    proposer: Signer,
    source: [u8; 16],
    history: Arc<dyn VersionedStore>,
    producer: bool,
) -> Launch {
    Launch {
        history: History::create(history, genesis.hash()).unwrap(),
        genesis: genesis.clone(),
        source,
        certificate: SignedHeader::sign(genesis, genesis.header(), &proposer).unwrap(),
        signer: producer.then_some(proposer),
    }
}

/// A host of `genesis` over fresh memory stores (see [`launch`]).
pub fn memory_host(
    genesis: &SignedGenesis,
    proposer: Signer,
    source: [u8; 16],
    producer: bool,
) -> Host {
    Host::create(
        Arc::new(MemoryVersionedStore::new()),
        Options::default(),
        launch(
            genesis,
            proposer,
            source,
            Arc::new(MemoryVersionedStore::new()),
            producer,
        ),
        Prefill::None,
    )
    .unwrap()
}

/// `operations` on the entity namespace.
pub fn arkiv_ops(operations: Vec<ArkivOperation>) -> Vec<TxOp> {
    operations
        .into_iter()
        .map(|operation| TxOp::Arkiv {
            namespace: arkiv::entity_namespace(),
            operation,
        })
        .collect()
}

/// A transaction of `signer` on the chain of `genesis`.
pub fn sign_tx(
    genesis: &SignedGenesis,
    signer: &Signer,
    nonce: u64,
    request_id: [u8; 32],
    tx_budget: u64,
    ops: Vec<TxOp>,
) -> SignedTx {
    let envelope = TxEnvelope {
        chain_id: genesis.genesis.chain_id,
        actor: signer.address(),
        request_id,
        tx_budget,
        ops,
    };
    SignedTx::sign(genesis, nonce, envelope, signer).unwrap()
}
