# Reserve-aware V2 paper simulation

## Canonical network inputs

- Robinhood Chain mainnet chain ID: `4663`
- Public RPC: `https://rpc.mainnet.chain.robinhood.com`
- Uniswap V2 factory: `0x8bceaa40b9acdfaedf85adf4ff01f5ad6517937f`
- Uniswap V2Router02: `0x89e5db8b5aa49aa85ac63f691524311aeb649eba`

Sources:

- https://docs.robinhood.com/chain/deploy-smart-contracts/
- https://developers.uniswap.org/docs/protocols/v2/deployments

## Ordering model

A follower transaction is expected to execute after the observed leader. The
simulator therefore:

1. loads all pair reserves from one pinned L2 block;
2. applies the leader path using the V2 `997 / 1000` fee formula;
3. mutates the in-memory reserves at every hop;
4. quotes the capped follower input against that post-leader state; and
5. rejects when the quoted follower output is below its proportional minimum.

Arithmetic is checked. Missing pairs, zero reserves, stale snapshots, overflow,
and outputs that round to zero fail closed.

## Live RPC validation

The official RPC returned chain ID `0x1237`, non-empty factory bytecode, and
resolved captured path
`0x0bd7...ad73 -> 0x7b69...ed76` to pair
`0xc3bc2c7c7def462191abaa0c73f8d5aede827d85`. Rust independently returned the
same pair, token order, reserves, and pinned block.

A fresh candidate trial completed pair discovery and reserve snapshotting in
627 ms, at sequence `7459945` versus snapshot block `7459948`. The simulator
produced a valid hop journal, but the three-sequence delay means the snapshot
could already include the leader. This proves that on-demand RPC belongs only
in validation and cold-cache recovery—not in the copy-trading hot path.

## Implemented hot-path design

The runtime must maintain reserves before a signal arrives:

```text
factory pair registry -> confirmed Sync-log updater -> in-memory reserve cache
                                                    -> feed candidate
                                                    -> clone cached path state
                                                    -> leader simulation
                                                    -> follower quote
                                                    -> paper journal
```

The original policy observer remains unchanged. A separate reserve-aware shadow
now runs alongside it, tails the same feed journal, and uses an immutable cache
binary. No wallet, signer, nonce manager, or transaction sender is present.
