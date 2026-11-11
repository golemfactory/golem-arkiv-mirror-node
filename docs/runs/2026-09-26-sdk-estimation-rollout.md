# Balance-independent gas estimation rollout, chain 9008 (2026-09-26)

Dated deployment record, moved from the SDK compatibility notes. Chain 9008 has
since been replaced.

Node change `bb688b2` is deployed to the producer, full follower and light
follower on chain 9008. Existing genesis, state/history, two-second production
and public metrics endpoints were retained. Followers were upgraded and caught
up before the producer; explicit downtime catch-up added 32 real blocks.

The exact previously failing wallet-6 create (5,000-byte payload) now estimates
`0x1b53c` (111,932 gas), identical to a funded caller, on all three public
endpoints over HTTP and WebSocket. Wallet-6 remains at zero balance and nonce.
Each endpoint rejected a genuinely signed unfunded EIP-1559 transaction with
`-32000` / `insufficient funds for gas * price + value`.

A funded signed copy submitted through the full follower committed in block
31794 with 100,848 gas used and 100,848,000,000,000 wei charged. All three nodes
returned the same successful receipt for transaction
`0x66ffd5f8e178ebb7d075040b0b322edc22670d054b2ef8b2b15e55ca4059a2c1`.
The pre-upgrade block 31611 retained its hash and state root on every role.
Thirteen parsed Prometheus scrapes per role over 60.5 seconds passed type and
unique-series checks with no new error counters. The producer advanced 31
blocks; all roles agreed on block 31823, its hash and state root afterward.

The deployed worker now fails at `eth_sendRawTransaction`, with the correct
insufficient-funds cause; it no longer fails during estimation. The SDK still
wraps non-revert contract errors in the generic outer message
`EntityMutationError: Transaction failed: Execution error without revert data`.
That presentation issue is separate from this RPC fix; company SDK/indexer
repositories and wallet funding were not changed.
