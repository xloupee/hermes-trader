# LaunchHood V3 and Pons identity audit (2026-07-16)

Scope: explain the `token_prediction_missing` and `pool_prediction_missing`
results in the quiet1 paper sample, and close only derivations that are
independently reproducible before receipt. This work remains paper-only.

## LaunchHood V3: pre-receipt prediction is proven

The Blockscout-verified source for factory
`0x62b33a039d289cbda50ebeb72fe4261449e61bcf` identifies the contract as
`LaunchHoodV3Factory` (Solidity 0.8.26). Its `launchToken` path deploys the
token with OpenZeppelin `Clones.cloneDeterministic`, using:

```text
implementation = TOKEN_IMPL
salt = keccak256(abi.encode(msg.sender, userSalt))
deployer = LaunchHoodV3Factory
```

Its public `predictTokenAddress` uses the same inputs. The factory's on-chain
`TOKEN_IMPL()` result on 2026-07-16 was
`0x5fdf73abc7a232d91b03638c2f9a52c16ab0e3be`, matching the checked-in
constant. Startup already requires the exact LaunchHood factory runtime pin;
the deterministic address is therefore derived locally from pinned factory
identity, the direct transaction sender, and canonical calldata. No
candidate-time RPC is needed.

The token address then determines the canonical 1% WETH V3 pool through the
already-pinned V3 factory and pool init-code hash. The implementation was
checked against all three quiet1 factory events:

| transaction | predicted/event token | predicted/event pool |
|---|---|---|
| `0x2359b9369b0efd158e9b9f387ecf3b685d01b49abb264015addc9e9d11cb9b4d` | `0x977b296fad263a990c439cfef548978155f8deb6` | `0xee74b862ea1640e8298015e5521f6046e32fce02` |
| `0x467f7a016d8e122be5f2e77e918f8016d5806f04ef15abc432caa00d81f22e6d` | `0xe20b86ad5729a50728aaf6423c8a95a97034741e` | `0xf7863134da38190bc4e39b1fe2a0bb2b60ffe82d` |
| `0x3dd255e79184b915eeb05e821bf55a22874876932f790983e44af816f9c4db4d` | `0xc3a9d7f871da5fa115fd3a82e5c23013e219ae75` | `0x90461bc66d008de3e050583d3bee03314b297361` |

The adapter now emits these identities as predictions while retaining
`execution_ready = false`. Its pure reconciliation helper requires both
factory-event identities and rejects missing, forged-token, or forged-pool
evidence.

Primary source:
[Blockscout getsourcecode for LaunchHoodV3Factory](https://robinhoodchain.blockscout.com/api?module=contract&action=getsourcecode&address=0x62B33A039D289CBDA50EBEb72fE4261449e61bcf)
(retrieved 2026-07-16).

## Pons: receipt identity is canonical; pre-receipt prediction is not proven

For Pons, the exact generation factory's `TokenLaunched` event supplies token
and pool identity. The receipt quoter additionally requires the canonical V3
`PoolCreated` evidence and generation-specific factory/locker provenance, so
the post-receipt identity is independently reconcilable and not inferred from
the observer claim. Quiet1's current-generation positive was transaction
`0x3315d471c1dbe03fdf01bfbe6f780dd406048c857eec7068872494dea074350f`,
token `0x3178130e61ddb3518b00c96ba99fa0c353fd85ae`, pool
`0x3ac3f5fce56ee52bc302f543e0bbadbb9bbf4155`.

That receipt evidence cannot honestly be backfilled as a pre-receipt
prediction. The current factory's verified creation construction / token
implementation formula has not been independently pinned, and 27 of 28
quiet1 Pons receipts were from the legacy generation. Consequently Pons stays
`DisabledIncompleteEvidence` with `UnresolvedUntilReceipt`; token and pool
prediction missing counts remain an intentional promotion blocker. Closing
that gap requires verified current-factory creation source or bytecode-derived
init-code semantics, an implementation/runtime pin, and multiple exact
calldata-to-receipt proofs plus forged-salt/factory negatives.
