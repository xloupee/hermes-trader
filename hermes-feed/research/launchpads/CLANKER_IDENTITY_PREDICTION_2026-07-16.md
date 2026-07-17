# Clanker pre-receipt identity prediction — 2026-07-16

Scope: prove the token CREATE2 and Uniswap v4 pool-ID formulas used by the
paper observer. This does not enable execution, signing, broadcasting, or a
canary.

## Source and bytecode authority

The Robinhood Blockscout verified factory source identifies
`0xd3f2...9a94` as `Clanker`, compiled with Solidity 0.8.28, optimizer runs
20,000, Cancun EVM. Its runtime hash remains the production pin
`0xf895112a2deed34ba2765d0147aff3494104a28293cc2f19af9275934088da33`.

The runtime delegates the `ClankerDeployer.deployToken` library call to
`0xfb2bae281d9f9d11ae3aed87bb717b058c9797e6`; because it is a
`DELEGATECALL`, CREATE2 executes in the pinned factory context. Blockscout
also verifies that library as `ClankerDeployer` and its observed runtime hash
is `0x90b7bf626c59dbc11e746825236f79693e2f3da80b2f551f59ab7b5030e5a3c4`.

Official `clanker-devco/v4-contracts` commit
`b004c2edda29fa282a16d5d1441a26484f70b37f` supplies the matching formula
and token constructor. The checked-in creation template was extracted from
the Blockscout-verified quiet1 token `0x44120e...0b07` by removing its exact
verified constructor arguments. It is 16,310 bytes and hashes to
`0xc3cf9289693d52fa53c127db0773c5eca16d8b29ab5c8b9aa9d3a72f39ad4815`.
Startup authenticates both values before candidate observation.

Primary sources:

- [Robinhood Blockscout verified factory](https://robinhoodchain.blockscout.com/api/v2/smart-contracts/0xd3f2cc1731b7fd17f28798835c2e02f0a1839a94)
- [Robinhood Blockscout verified deployer library](https://robinhoodchain.blockscout.com/api?module=contract&action=getsourcecode&address=0xfb2bae281d9f9d11ae3aed87bb717b058c9797e6)
- [Robinhood Blockscout verified quiet1 token bytecode](https://robinhoodchain.blockscout.com/api/v2/smart-contracts/0x44120ed6e3eeba3c5b504e96636ebfd18fef0b07)
- [Official ClankerDeployer source at b004c2e](https://github.com/clanker-devco/v4-contracts/blob/b004c2edda29fa282a16d5d1441a26484f70b37f/src/utils/ClankerDeployer.sol)
- [Official ClankerToken source at b004c2e](https://github.com/clanker-devco/v4-contracts/blob/b004c2edda29fa282a16d5d1441a26484f70b37f/src/ClankerToken.sol)
- [Official ClankerHookV2 source at b004c2e](https://github.com/clanker-devco/v4-contracts/blob/b004c2edda29fa282a16d5d1441a26484f70b37f/src/hooks/ClankerHookV2.sol)

## Exact formulas

```text
create2Salt = keccak256(abi.encode(tokenAdmin, userSalt))

constructorArgs = abi.encode(
  name,
  symbol,
  100_000_000_000e18,
  tokenAdmin,
  image,
  metadata,
  context,
  originatingChainId
)

initCode = pinnedClankerTokenCreationCode || constructorArgs
token = last20(keccak256(0xff || factory || create2Salt || keccak256(initCode)))
```

The hook sorts the predicted token and calldata `pairedToken`, sets fee to
the Uniswap v4 dynamic-fee flag `0x800000`, and retains calldata tick spacing
and hook:

```text
poolId = keccak256(abi.encode(currency0, currency1, 0x800000, tickSpacing, hook))
```

## Quiet1 reproduction

| Transaction | Predicted and receipt token | Predicted and receipt pool ID |
|---|---|---|
| `0xf418847e...8ea46` | `0x44120ed6e3eeba3c5b504e96636ebfd18fef0b07` | `0x2e392459b4eeb9d29b72e5233f70caa66f1f3fe99261104d90c0c478afd8c2ef` |
| `0x9f16b61e...c8ddf` | `0x938c5d02df35e3b448ed2bb954e0f3fb7627eb07` | `0xba0bf6ba09b265308ef880a8ac20bd4cef8f3529478259033bfa8232988f0835` |
| `0x36131324...e3d81` | `0x68b654c0ba2794c9ef00b23b0bebf40bc7122b07` | `0x4f58cd406b5de13e5535933af3cffaaaf2f14f28408d9f919db585035fa8691c` |

Tests bind every constructor field through exact ABI encoding, exercise
changed token-admin, salt, context, hook, and paired-token neighbors, and
reject forged or missing receipt token/pool identities. Predictions are added
to paper observations, while `ExecutionMode::ExecutionGated` and
`live_execution_enabled = false` remain unchanged.
