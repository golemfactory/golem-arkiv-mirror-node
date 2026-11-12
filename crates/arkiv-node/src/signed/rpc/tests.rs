use super::super::{
    crypto::Signer,
    ethereum,
    history::History,
    pool::DropReason,
    protocol::{SignedGenesis, SignedHeader, SignedTx},
};
use super::{
    blocks::{ZERO_HASH, receipt},
    entities::entity_nonce,
    filters::FILTER_TTL,
    logs::{logs, topic_address},
    params::{bytes, uint, word},
    simulate::{Simulation, revert_data, simulation},
    *,
};
use crate::{
    TxOp, TxStatus,
    arkiv::{self, ArkivOperation},
};
use alloy_primitives::{U256, keccak256};
use arkiv_db::{CellName, Value as DbValue, durable::Options};
use arkiv_db::{OwnerId, RecordKey};
use arkiv_db_memory::MemoryVersionedStore;
use std::sync::Arc;

fn refused(result: std::result::Result<Value, ReadError>) -> &'static str {
    match result {
        Err(ReadError::Node(e)) => e.code.as_str(),
        other => panic!("expected a node error, got {other:?}"),
    }
}

fn host() -> Host {
    host_with(crate::signed::protocol::MAX_EXPIRY_PER_BLOCK)
}

fn host_with(expiry_per_block: u32) -> Host {
    let signer = Signer::from_bytes([2; 32]).unwrap();
    let mut native = crate::signed::protocol::default_native_genesis(9300, 1000, 1000);
    native.params.expiry_per_block = expiry_per_block;
    let g = SignedGenesis::new(
        native,
        [7; 16],
        *signer.address().as_bytes(),
        1000,
        crate::accounts::AccountConfig::new(vec![(
            signer.address(),
            U256::from(10_000_000_000_000_000_000u64),
        )])
        .unwrap(),
    )
    .unwrap();
    let certificate = SignedHeader::sign(&g, g.header(), &signer).unwrap();
    let history = History::create(Arc::new(MemoryVersionedStore::new()), g.hash()).unwrap();
    let mut host = Host::create(
        Arc::new(MemoryVersionedStore::new()),
        Options::default(),
        crate::signed::host::Launch {
            history,
            genesis: g,
            source: [1; 16],
            certificate,
            signer: Some(signer),
        },
        crate::signed::host::Prefill::None,
    )
    .unwrap();
    host.produce().unwrap();
    host
}

fn native(host: &Host, nonce: u64, ops: Vec<ArkivOperation>) -> SignedTx {
    let signer = Signer::from_bytes([2; 32]).unwrap();
    let mut id = [0; 32];
    id[..8].copy_from_slice(&nonce.to_be_bytes());
    SignedTx::sign(
        &host.genesis,
        nonce,
        crate::TxEnvelope {
            chain_id: 9300,
            actor: signer.address(),
            request_id: id,
            tx_budget: 2_000_000,
            ops: ops
                .into_iter()
                .map(|operation| TxOp::Arkiv {
                    namespace: arkiv::entity_namespace(),
                    operation,
                })
                .collect(),
        },
        &signer,
    )
    .unwrap()
}

/// A create that finds every height of the spill search full reverts
/// with a Solidity `Error(string)` naming the height and the capacity.
#[test]
fn a_full_expiry_search_reverts_with_a_readable_reason() {
    let mut h = host_with(1);
    let create = |salt: u128| ArkivOperation::Create {
        salt,
        flags: 0,
        expiry: crate::Expiry {
            expires_at: 50,
            min_lifetime: 0,
        },
        attributes: vec![],
    };
    // One entity per height: 65 creates fill 50 ..= 114.
    for nonce in 0..5u64 {
        let ops = (0..13)
            .map(|i| create(u128::from(nonce * 13 + i)))
            .collect();
        h.submit(native(&h, 1 + nonce, ops)).unwrap();
    }
    let refused = native(&h, 6, vec![create(100), create(101)]);
    h.submit(refused.clone()).unwrap();
    h.produce().unwrap();
    let block = h.block(2).unwrap();
    assert!(
        block.outcomes.txs[..5]
            .iter()
            .all(|tx| tx.status == TxStatus::Committed)
    );
    // Events report the heights given: 50, 51, ... in creation order.
    let logs = block_logs(&h, 2).unwrap();
    assert_eq!(logs.len(), 65);
    for (i, log) in logs.iter().enumerate() {
        assert_eq!(log["data"], format!("0x{}{}", word(50 + i as u64), word(0)));
    }
    let out = &block.outcomes.txs[5];
    assert_eq!(out.status, TxStatus::Failed { op_index: 0 });
    assert_eq!(receipt(&h, 2, 5).unwrap()["status"], "0x0");
    let data = revert_data(&h, &refused.tx, out, 2).unwrap().unwrap();
    assert_eq!(&data[..10], "0x08c379a0");
    let raw = bytes(&json!(data), None).unwrap();
    let len = usize::from_be_bytes(raw[60..68].try_into().unwrap());
    assert_eq!((raw.len() - 4) % 32, 0);
    assert_eq!(
        std::str::from_utf8(&raw[68..68 + len]).unwrap(),
        "Arkiv: operation 0 cannot expire at block 50: blocks 50 to 114 each already have \
             the 1 expiring entities one block may have; choose another expiry"
    );
}

#[test]
fn log_owners_follow_operations_and_failed_batches_emit_nothing() {
    let mut h = host();
    let caller = *Signer::from_bytes([2; 32]).unwrap().address().as_bytes();
    let key = arkiv::derive_entity_key(9300, &caller, 0, 3);
    let new = [4; 20];
    let create = ArkivOperation::Create {
        salt: 3,
        flags: 0,
        expiry: crate::Expiry {
            expires_at: 0,
            min_lifetime: 20,
        },
        attributes: vec![(
            CellName::new("data").unwrap(),
            arkiv_db::Cell::field(DbValue::bytes(b"test")),
        )],
    };
    let tx = native(
        &h,
        1,
        vec![
            create,
            ArkivOperation::TransferOwnership {
                key: RecordKey::new(&key).unwrap(),
                owner: new,
            },
        ],
    );
    h.submit(tx).unwrap();
    h.produce().unwrap();
    let events = block_logs(&h, 2).unwrap();
    assert_eq!(events.len(), 2);
    assert_eq!(events[0]["topics"][1], hex(&key));
    assert_eq!(events[0]["data"], format!("0x{}{}", word(22), word(0)));
    assert_eq!(events[1]["topics"][2], topic_address(&caller));
    assert_eq!(events[1]["topics"][3], topic_address(&new));
    let bad = native(
        &h,
        2,
        vec![
            ArkivOperation::Create {
                salt: 4,
                flags: 0,
                expiry: crate::Expiry {
                    expires_at: 0,
                    min_lifetime: 21,
                },
                attributes: vec![],
            },
            ArkivOperation::Delete {
                key: RecordKey::new(&[9; 32]).unwrap(),
            },
        ],
    );
    h.submit(bad).unwrap();
    h.produce().unwrap();
    assert!(block_logs(&h, 3).unwrap().is_empty());
    assert_eq!(receipt(&h, 3, 0).unwrap()["status"], "0x0");
    assert_eq!(entity_nonce(&h, 3, &caller).unwrap(), 1);
    // The receipt carries the burned rent beside the gas: the committed
    // create's, and none for the rolled-back batch.
    let rent = h.block(2).unwrap().outcomes.txs[0].rent_wei;
    assert!(rent > U256::ZERO);
    let paid = receipt(&h, 2, 0).unwrap();
    assert_eq!(paid["rentFee"], format!("{rent:#x}"));
    assert_eq!(paid["status"], "0x1");
    assert_eq!(receipt(&h, 3, 0).unwrap()["rentFee"], "0x0");
}

#[tokio::test]
async fn a_dropped_transaction_answers_with_a_definite_error() {
    let mut h = host();
    let caller = Signer::from_bytes([2; 32]).unwrap().address();
    let tx = native(
        &h,
        1,
        vec![ArkivOperation::Create {
            salt: 9,
            flags: 0,
            expiry: crate::Expiry {
                expires_at: 0,
                min_lifetime: 20,
            },
            attributes: vec![],
        }],
    );
    let hash = hex(tx.hash().as_slice());
    let request = tx.tx.request_id;
    h.submit(tx).unwrap();
    assert_eq!(
        h.pool
            .evict_from(caller, 1, DropReason::InsufficientBalance),
        1
    );
    assert_eq!(
        h.request_status(caller, request).unwrap()["status"],
        "dropped"
    );
    let rt =
        super::super::runtime::Runtime::new(Node::Full(Box::new(h)), None, None, None).unwrap();
    for method in ["eth_getTransactionReceipt", "eth_getTransactionByHash"] {
        let answer = handle_request(
            &rt,
            json!({"jsonrpc":"2.0","id":1,"method":method,"params":[hash]}),
        )
        .await;
        assert_eq!(
            answer["error"],
            json!({
                "code": -32000,
                "message": concat!(
                    "transaction dropped: ",
                    "insufficient funds for gas * price + value + rent",
                ),
            }),
            "{method}"
        );
    }
    // An unknown hash is still just absent.
    let unknown = handle_request(
        &rt,
        json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "eth_getTransactionReceipt",
            "params": [ZERO_HASH],
        }),
    )
    .await;
    assert_eq!(unknown["result"], Value::Null);
}

#[test]
fn unfunded_estimates_match_execution_gas_without_mutating_state() {
    let mut h = host();
    let funded = Signer::from_bytes([2; 32]).unwrap().address();
    let empty = Signer::from_bytes([3; 32]).unwrap().address();
    let vectors: Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/ethereum-sdk-vectors.json"
    ))
    .unwrap();
    let before = h.head;
    let snapshot = h.engine.snapshot(h.engine.head()).unwrap();
    let burned = snapshot.try_host_get(&crate::accounts::BURN_KEY).unwrap();
    let balances = [funded, empty].map(|a| h.account(h.head.height, a).unwrap());
    let mut state = RpcState::default();
    let mut call = json!({"from":hex(empty.as_bytes()),"to":ADDRESS,"data":vectors["data"]});
    let estimate = dispatch(
        &mut h,
        &mut state,
        "eth_estimateGas",
        &json!([call, "pending"]),
    )
    .unwrap();
    let (used, _) = simulation(&h, &call, Simulation::Estimate).unwrap();
    assert_eq!(uint(&estimate).unwrap(), used + used / 10 + 1000);
    // A returned estimate must itself be sufficient, including rent.
    call["gas"] = estimate.clone();
    assert_eq!(simulation(&h, &call, Simulation::Estimate).unwrap().0, used);
    // eth_call retains its existing account rules.
    assert_eq!(
        refused(dispatch(&mut h, &mut state, "eth_call", &json!([call]))),
        "InsufficientBalance"
    );
    call["from"] = json!(hex(funded.as_bytes()));
    assert_eq!(
        dispatch(&mut h, &mut state, "eth_estimateGas", &json!([call])).unwrap(),
        estimate
    );
    assert_eq!(simulation(&h, &call, Simulation::Call).unwrap().0, used);
    call["gas"] = json!("0x1");
    assert_eq!(
        refused(dispatch(
            &mut h,
            &mut state,
            "eth_estimateGas",
            &json!([call])
        )),
        "ExecutionReverted"
    );
    assert_eq!(h.head, before);
    assert_eq!(
        h.engine
            .snapshot(h.engine.head())
            .unwrap()
            .try_host_get(&crate::accounts::BURN_KEY)
            .unwrap(),
        burned
    );
    assert!(h.pool.is_empty());
    for (a, balance) in [funded, empty].into_iter().zip(balances) {
        assert_eq!(h.account(h.head.height, a).unwrap(), balance);
        assert_eq!(entity_nonce(&h, h.head.height, a.as_bytes()).unwrap(), 0);
    }
    let operations = ethereum::operations(&bytes(&vectors["data"], None).unwrap()).unwrap();
    h.submit(native(&h, 1, operations)).unwrap();
    h.produce().unwrap();
    assert_eq!(h.block(2).unwrap().outcomes.txs[0].spent, used);
}

#[test]
fn transfer_estimation_ignores_fee_and_value_affordability() {
    let mut h = host();
    let empty = Signer::from_bytes([3; 32]).unwrap().address();
    let recipient = Signer::from_bytes([4; 32]).unwrap().address();
    let before = h.head;
    let mut state = RpcState::default();
    for value in [U256::ZERO, U256::from(1), U256::MAX] {
        for to in [empty, recipient] {
            let call = json!({
                "from": hex(empty.as_bytes()),
                "to": hex(to.as_bytes()),
                "value": format!("{value:#x}"),
            });
            assert_eq!(
                dispatch(&mut h, &mut state, "eth_estimateGas", &json!([call])).unwrap(),
                quantity(21_000)
            );
        }
    }
    assert_eq!(h.head, before);
    assert!(h.pool.is_empty());
    for a in [empty, recipient] {
        assert_eq!(h.account(h.head.height, a).unwrap().balance, U256::ZERO);
        assert_eq!(h.account(h.head.height, a).unwrap().nonce, 0);
    }
}

fn signed_transfer(signer: &Signer, nonce: u64, to: OwnerId, value: U256) -> Value {
    signed_transfer_with_gas(signer, nonce, to, value, 100_000, false)
}

fn signed_transfer_with_gas(
    signer: &Signer,
    nonce: u64,
    to: OwnerId,
    value: U256,
    gas: u64,
    typed: bool,
) -> Value {
    use alloy_rlp::Encodable;
    let list = |payload: Vec<u8>| {
        let mut encoded = Vec::new();
        alloy_rlp::Header {
            list: true,
            payload_length: payload.len(),
        }
        .encode(&mut encoded);
        encoded.extend(payload);
        encoded
    };
    let mut payload = Vec::new();
    if typed {
        9300u64.encode(&mut payload);
    }
    nonce.encode(&mut payload);
    if typed {
        0u8.encode(&mut payload);
    }
    crate::accounts::GAS_PRICE_WEI.encode(&mut payload);
    gas.encode(&mut payload);
    to.as_bytes().as_slice().encode(&mut payload);
    value.encode(&mut payload);
    (&[] as &[u8]).encode(&mut payload);
    if typed {
        payload.extend(list(Vec::new()));
        let mut signing = vec![2];
        signing.extend(list(payload.clone()));
        let sig = signer.sign_digest(keccak256(signing)).unwrap();
        sig[64].encode(&mut payload);
        U256::from_be_slice(&sig[..32]).encode(&mut payload);
        U256::from_be_slice(&sig[32..64]).encode(&mut payload);
        let mut raw = vec![2];
        raw.extend(list(payload));
        return json!(hex(&raw));
    }
    let mut unsigned = payload.clone();
    9300u64.encode(&mut unsigned);
    0u8.encode(&mut unsigned);
    0u8.encode(&mut unsigned);
    let sig = signer.sign_digest(keccak256(list(unsigned))).unwrap();
    (9300 * 2 + 35 + sig[64] as u64).encode(&mut payload);
    U256::from_be_slice(&sig[..32]).encode(&mut payload);
    U256::from_be_slice(&sig[32..64]).encode(&mut payload);
    json!(hex(&list(payload)))
}

#[test]
fn legacy_and_eip1559_transfers_execute_with_exact_intrinsic_gas() {
    for typed in [false, true] {
        let mut h = host();
        let signer = Signer::from_bytes([2; 32]).unwrap();
        let recipient = Signer::from_bytes([4; 32]).unwrap().address();
        let before = h.account(h.head.height, signer.address()).unwrap();
        let mut state = RpcState::default();
        let call = json!({
            "from": hex(signer.address().as_bytes()),
            "to": hex(recipient.as_bytes()),
            "value": "0x1",
            "gas": "0x5208",
        });
        assert_eq!(
            dispatch(&mut h, &mut state, "eth_estimateGas", &json!([call])).unwrap(),
            quantity(21_000)
        );
        let raw = signed_transfer_with_gas(
            &signer,
            before.nonce,
            recipient,
            U256::from(1),
            21_000,
            typed,
        );
        let hash = dispatch(&mut h, &mut state, "eth_sendRawTransaction", &json!([raw])).unwrap();
        h.produce().unwrap();
        let result = dispatch(
            &mut h,
            &mut state,
            "eth_getTransactionReceipt",
            &json!([hash]),
        )
        .unwrap();
        assert_eq!(result["status"], "0x1");
        assert_eq!(result["gasUsed"], "0x5208");
        assert_eq!(
            h.account(h.head.height, signer.address()).unwrap().balance,
            before.balance - U256::from(1) - crate::accounts::fee(21_000)
        );
        assert_eq!(
            h.account(h.head.height, recipient).unwrap().balance,
            U256::from(1)
        );
        assert_eq!(
            h.account(h.head.height, signer.address()).unwrap().nonce,
            before.nonce + 1
        );
    }
}

#[tokio::test]
async fn raw_send_enforces_balance_and_pending_reservations_after_estimation() {
    let h = host();
    let signer = Signer::from_bytes([3; 32]).unwrap();
    let actor = signer.address();
    let recipient = Signer::from_bytes([4; 32]).unwrap().address();
    let raw = signed_transfer(&signer, 0, recipient, U256::from(1));
    let rt =
        super::super::runtime::Runtime::new(Node::Full(Box::new(h)), None, None, None).unwrap();
    let request = |method, params| json!({"jsonrpc":"2.0","id":7,"method":method,"params":params});
    let call = json!({"from":hex(actor.as_bytes()),"to":hex(recipient.as_bytes()),"value":"0x1"});
    let estimate = handle_request(&rt, request("eth_estimateGas", json!([call]))).await;
    assert_eq!(estimate["result"], quantity(21_000));
    let rejected = handle_request(&rt, request("eth_sendRawTransaction", json!([raw]))).await;
    assert_eq!(
        rejected,
        json!({
            "jsonrpc": "2.0",
            "id": 7,
            "error": {
                "code": -32000,
                "message": "insufficient funds for gas * price + value + rent",
            },
        })
    );
    {
        let mut guard = rt.node.lock().unwrap();
        let Node::Full(h) = &mut *guard else {
            panic!("full node")
        };
        assert!(h.pool.is_empty());
        assert_eq!(h.account(h.head.height, actor).unwrap().nonce, 0);
        assert_eq!(h.account(h.head.height, actor).unwrap().balance, U256::ZERO);
        // Fund just one reservation, far below the default simulation gas cap.
        let fund = crate::TxEnvelope {
            chain_id: 9300,
            actor: Signer::from_bytes([2; 32]).unwrap().address(),
            request_id: [42; 32],
            tx_budget: 100_000,
            ops: vec![TxOp::Transfer {
                to: actor,
                amount: crate::accounts::fee(100_000) + U256::from(1),
            }],
        };
        h.submit(
            SignedTx::sign(&h.genesis, 1, fund, &Signer::from_bytes([2; 32]).unwrap()).unwrap(),
        )
        .unwrap();
        h.produce().unwrap();
    }
    let accepted = handle_request(&rt, request("eth_sendRawTransaction", json!([raw]))).await;
    assert!(accepted["result"].is_string(), "{accepted}");
    assert_eq!(
        handle_request(&rt, request("eth_estimateGas", json!([call]))).await["result"],
        estimate["result"]
    );
    let second = signed_transfer(&signer, 1, recipient, U256::from(1));
    assert_eq!(
        handle_request(&rt, request("eth_sendRawTransaction", json!([second]))).await,
        rejected
    );
    let mut guard = rt.node.lock().unwrap();
    let Node::Full(h) = &mut *guard else {
        panic!("full node")
    };
    assert_eq!(h.pool.len(), 1);
    h.produce().unwrap();
    assert_eq!(h.account(h.head.height, actor).unwrap().nonce, 1);
    assert_eq!(
        h.account(h.head.height, actor).unwrap().balance,
        crate::accounts::fee(79_000)
    );
    assert_eq!(
        h.account(h.head.height, recipient).unwrap().balance,
        U256::from(1)
    );
}

#[test]
fn quantities_are_minimal_hex_without_a_sign() {
    assert_eq!(uint(&json!("0x0")).unwrap(), 0);
    assert_eq!(uint(&json!("0x2a")).unwrap(), 42);
    let h = host();
    let who = hex(Signer::from_bytes([2; 32]).unwrap().address().as_bytes());
    for refused in ["0x+1", "0x", "0x01"] {
        assert!(uint(&json!(refused)).is_err(), "{refused}");
        let call = json!({"from": who, "to": who, "value": refused});
        assert_eq!(
            simulation(&h, &call, Simulation::Call).unwrap_err().code,
            ErrorCode::InvalidParams,
            "{refused}"
        );
    }
}

#[test]
fn malformed_hex_and_log_ranges_are_rejected_without_panics() {
    for s in ["0x€a", "0xéé", "0xz0", "0x1", "0X11"] {
        assert!(bytes(&json!(s), None).is_err());
    }
    assert!(
        logs(
            &host(),
            &mut RpcState::default(),
            &json!({"fromBlock":"0x1","toBlock":"0x0"})
        )
        .is_err()
    );
}

#[tokio::test]
async fn jsonrpc_batches_notifications_and_errors_keep_protocol_shape() {
    let rt = super::super::runtime::Runtime::new(Node::Full(Box::new(host())), None, None, None)
        .unwrap();
    let response = handle_request(
        &rt,
        json!([
            {"jsonrpc": "2.0", "id": 1, "method": "eth_chainId"},
            {"jsonrpc": "2.0", "method": "eth_chainId"},
            {"jsonrpc": "2.0", "id": 2, "method": "unknown"},
        ]),
    )
    .await;
    assert_eq!(response.as_array().unwrap().len(), 2);
    assert_eq!(response[0]["result"], "0x2454");
    assert_eq!(response[1]["error"]["code"], -32601);
    let bad = handle_request(&rt, json!({"jsonrpc":"2.0","id":{},"method":"eth_chainId"})).await;
    assert_eq!(bad["error"]["code"], -32600);
    let notification = handle_request(&rt, json!({"jsonrpc":"2.0","method":"unknown"})).await;
    assert!(notification.is_null());
}

#[tokio::test]
async fn concurrent_rpc_receipt_reads_do_not_use_rest_admission_slots() {
    let rt = super::super::runtime::Runtime::new(Node::Full(Box::new(host())), None, None, None)
        .unwrap();
    // Saturate native REST admission while a scanner-sized HTTP burst runs.
    let _rest_permits = rt.slots.clone().acquire_many_owned(4).await.unwrap();
    let mut requests = tokio::task::JoinSet::new();
    for id in 0..32 {
        let rt = rt.clone();
        requests.spawn(async move {
            let response = http(
                State(rt),
                Body::from(
                    json!({
                        "jsonrpc": "2.0",
                        "id": id,
                        "method": "eth_getTransactionReceipt",
                        "params": [ZERO_HASH],
                    })
                    .to_string(),
                ),
            )
            .await;
            assert_eq!(response.status(), StatusCode::OK);
            let body = to_bytes(response.into_body(), 4096).await.unwrap();
            let value: Value = serde_json::from_slice(&body).unwrap();
            assert_eq!(value, json!({"jsonrpc":"2.0","id":id,"result":null}));
        });
    }
    tokio::time::timeout(std::time::Duration::from_secs(10), async {
        while let Some(result) = requests.join_next().await {
            result.unwrap();
        }
    })
    .await
    .unwrap();
}

#[test]
fn filters_reject_unknown_fields_and_bound_every_retained_dimension() {
    let address = hex(&[1; 20]);
    let topic = hex(&[2; 32]);
    validate_filter(&json!({
        "address": [address],
        "topics": [null, [topic, null]],
        "fromBlock": "earliest",
        "toBlock": "0xffffffffffffffff",
    }))
    .unwrap();
    for bad in [
        json!({"typo":true}),
        json!({"address":null}),
        json!({"address":topic}),
        json!({"address":vec![address;65]}),
        json!({"topics":null}),
        json!({"topics":vec![Value::Null;5]}),
        json!({"topics":[vec![Value::Null;65]]}),
        json!({"topics":["0x01"]}),
        json!({"fromBlock":false}),
        json!({"fromBlock":"0x00000000000000000001"}),
        json!({"blockHash":ZERO_HASH,"toBlock":"latest"}),
        json!({"blockHash":ZERO_HASH,"fromBlock":"0x0"}),
    ] {
        assert!(validate_filter(&bad).is_err(), "{bad}");
        let mut h = host();
        let mut state = RpcState::default();
        assert!(logs(&h, &mut state, &bad).is_err());
        assert!(dispatch(&mut h, &mut state, "eth_newFilter", &json!([bad])).is_err());
        assert!(state.filters.is_empty());
    }
}

fn create_filter_event(h: &mut Host, nonce: u64) {
    let tx = native(
        h,
        nonce,
        vec![ArkivOperation::Create {
            salt: nonce as u128,
            flags: 0,
            expiry: crate::Expiry {
                expires_at: 0,
                min_lifetime: 20,
            },
            attributes: vec![],
        }],
    );
    h.submit(tx).unwrap();
    h.produce().unwrap();
}

#[test]
fn finite_and_block_hash_filters_keep_bounds_and_poll_without_duplicates() {
    let mut h = host();
    let mut state = RpcState::default();
    create_filter_event(&mut h, 1);
    create_filter_event(&mut h, 2);
    let hash = hex(h.block(2).unwrap().hash().as_slice());
    for spec in [
        json!({"fromBlock":"0x2","toBlock":"0x2"}),
        json!({"blockHash":hash}),
    ] {
        let id = dispatch(&mut h, &mut state, "eth_newFilter", &json!([spec])).unwrap();
        let first = dispatch(&mut h, &mut state, "eth_getFilterChanges", &json!([id])).unwrap();
        assert_eq!(first.as_array().unwrap().len(), 1);
        assert_eq!(first[0]["blockNumber"], "0x2");
        assert_eq!(
            dispatch(&mut h, &mut state, "eth_getFilterChanges", &json!([id])).unwrap(),
            json!([])
        );
        let all = dispatch(&mut h, &mut state, "eth_getFilterLogs", &json!([id])).unwrap();
        assert_eq!(all, first);
    }
    let id = dispatch(
        &mut h,
        &mut state,
        "eth_newFilter",
        &json!([{"fromBlock":"0x5","toBlock":"0x5"}]),
    )
    .unwrap();
    assert_eq!(
        dispatch(&mut h, &mut state, "eth_getFilterChanges", &json!([id])).unwrap(),
        json!([])
    );
    assert_eq!(
        dispatch(&mut h, &mut state, "eth_getFilterLogs", &json!([id])).unwrap(),
        json!([])
    );
}

#[test]
fn filters_expire_release_capacity_and_refresh_on_poll() {
    let mut h = host();
    let mut state = RpcState::default();
    let id = dispatch(&mut h, &mut state, "eth_newFilter", &json!([{}])).unwrap();
    let numeric = uint(&id).unwrap();
    state.filters.get_mut(&numeric).unwrap().touched = std::time::Instant::now() - FILTER_TTL;
    assert_eq!(
        refused(dispatch(
            &mut h,
            &mut state,
            "eth_getFilterChanges",
            &json!([id])
        )),
        "FilterNotFound"
    );
    assert!(state.filters.is_empty());
    let id = dispatch(&mut h, &mut state, "eth_newBlockFilter", &json!([])).unwrap();
    let numeric = uint(&id).unwrap();
    state.filters.get_mut(&numeric).unwrap().touched = std::time::Instant::now() - FILTER_TTL / 2;
    let before = std::time::Instant::now();
    dispatch(&mut h, &mut state, "eth_getFilterChanges", &json!([id])).unwrap();
    assert!(state.filters[&numeric].touched >= before);
    state.filters.get_mut(&numeric).unwrap().touched = before - FILTER_TTL;
    dispatch(&mut h, &mut state, "eth_newFilter", &json!([{}])).unwrap();
    assert!(!state.filters.contains_key(&numeric));
    assert_eq!(state.filters.len(), 1);
}

#[tokio::test]
async fn execute_call_returns_abi_keys_and_missing_entity_errors_decode() {
    use alloy_sol_types::{SolCall, SolValue};
    let h = host();
    let who = Signer::from_bytes([2; 32]).unwrap().address();
    let vectors: Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/ethereum-sdk-vectors.json"
    ))
    .unwrap();
    let (_, output) = simulation(
        &h,
        &json!({"from":hex(who.as_bytes()),"to":ADDRESS,"data":vectors["data"]}),
        Simulation::Call,
    )
    .unwrap();
    let expected = arkiv::derive_entity_key(9300, who.as_bytes(), 0, 9);
    assert_eq!(
        output,
        format!("0x{}{}{}", word(32), word(1), &hex(&expected)[2..])
    );
    assert_eq!(entity_nonce(&h, h.head.height, who.as_bytes()).unwrap(), 0);
    let data = ethereum::executeCall {
        ops: vec![ethereum::Operation {
            operation: 5,
            operationData: ethereum::Delete {
                entityKey: alloy_primitives::B256::from([0x11; 32]),
            }
            .abi_encode()
            .into(),
        }],
    }
    .abi_encode();
    let rt =
        super::super::runtime::Runtime::new(Node::Full(Box::new(h)), None, None, None).unwrap();
    let empty = Signer::from_bytes([3; 32]).unwrap().address();
    let result = handle_request(
        &rt,
        json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "eth_estimateGas",
            "params": [{"from": hex(empty.as_bytes()), "to": ADDRESS, "data": hex(&data)}],
        }),
    )
    .await;
    assert_eq!(result["error"]["code"], 3);
    assert_eq!(
        result["error"]["data"],
        format!(
            "{}{}",
            hex(&keccak256(b"EntityNotFound(bytes32)")[..4]),
            "11".repeat(32)
        )
    );
}

#[tokio::test]
async fn sdk_query_refusals_answer_with_the_sdk_code_table() {
    let rt = super::super::runtime::Runtime::new(Node::Full(Box::new(host())), None, None, None)
        .unwrap();
    let call = |params: Value| {
        handle_request(
            &rt,
            json!({"jsonrpc":"2.0","id":1,"method":"arkiv_query","params":params}),
        )
    };
    for (params, code) in [
        (json!(["n ="]), -32001),
        (json!(["n STARTSWITH u64(1)"]), -32002),
        (json!(["n = i32(2147483648)"]), -32003),
        (json!([format!("{}a = true", "NOT ".repeat(40))]), -32004),
        (
            json!(["*", {"atBlock":"0x0","cursor":format!("0x{}", "00".repeat(32))}]),
            -32005,
        ),
        (json!(["*", {"atBlock":"0xffff"}]), -32006),
    ] {
        let answer = call(params.clone()).await;
        assert_eq!(answer["error"]["code"], code, "{params}: {answer}");
    }
    let parse = call(json!(["n ="])).await;
    assert_eq!(
        parse["error"]["data"],
        json!({"position":3,"expected":"a value","got":"end of query"})
    );
    // `payload` and `contentType` are unknown select keys: invalid params.
    for removed in ["payload", "contentType"] {
        let answer = call(json!(["*", {"select": {"key": true, removed: true}}])).await;
        assert_eq!(answer["error"]["code"], -32602, "{answer}");
    }
    // No other error uses the query table.
    assert!(!(-32006..=-32001).contains(&SERVER_BUSY_CODE));
    assert!(!(-32006..=-32001).contains(&super::super::rpc_ws::SUBSCRIPTION_LIMIT_CODE));
}

#[test]
fn fee_history_rejects_allocation_amplification_before_building_rewards() {
    let mut h = host();
    let mut state = RpcState::default();
    assert_eq!(
        refused(dispatch(
            &mut h,
            &mut state,
            "eth_feeHistory",
            &json!(["0x400", "latest", vec![0; 101]])
        )),
        "InvalidParams"
    );
    for values in [json!([2, 1]), json!([-1]), json!([101]), json!(["0"])] {
        assert_eq!(
            refused(dispatch(
                &mut h,
                &mut state,
                "eth_feeHistory",
                &json!(["0x1", "latest", values])
            )),
            "InvalidParams"
        );
    }
}
