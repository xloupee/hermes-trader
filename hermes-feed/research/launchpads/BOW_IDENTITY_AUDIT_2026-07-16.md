# Bow pre-receipt identity audit (2026-07-16)

Scope: determine whether Bow token and pool identity can be reproduced before
receipt without candidate-time RPC. This change remains paper-only and does
not enable signing, broadcasting, or execution.

## Authority and source boundary

Bow's official developer guide identifies factory
`0xc70e510e14710ea535cab7b2414860af63feab79`, chain ID 4663, selector
`launch((...))`, `tokenInitCodeHash(params, creator)`, and
`predictToken(salt, initCodeHash)`. It instructs callers to compute the init
hash, mine the `params.salt`, and derive the CREATE2 address from the factory,
salt, and init hash.

The factory's Solidity source is **not verified** on Blockscout and no public
source repository was found. The local derivation therefore does not claim
source-build reproducibility. Its authority is the exact deployed bytecode:

- production runtime hash:
  `0x8d56cbcdf72dbf04ed8170d55878cc894997ccc54c2ab0aec782274eb7fe7a14`;
- runtime length: 16,318 bytes;
- `tokenInitCodeHash` selector: `0x79fac920`;
- token creation-code copy: runtime offset `0x20e8`, length `0x1ea1` (7,841
  bytes);
- creation-code hash:
  `0x31f06442f2a00efc42dba795fef6459a1e88f3ba8447ceaea56b084a8d8414fa`;
- compiler metadata trailer reports Solidity 0.8.30 and IPFS metadata CID
  `QmTzjvoHLaVyyQYpbb5Th1MSbK5FeFkA4JVkqu9qrJT7eQ`.

Disassembly of the pure selector path shows that the copied creation code is
followed by canonical ABI constructor parameters:

```text
(string name, string symbol, uint256 totalSupply, address creator,
 uint256 maxWallet, uint256 limitWindow)
```

Thus:

```text
initHash = keccak256(tokenCreationCode || abi.encode(constructor parameters))
token = CREATE2(factory, params.salt, initHash)
pool = canonicalV3(factory, sort(token, WETH), fee=10000, pinnedPoolInitHash)
```

`launchDelay`, `targetFdvWeth`, salt, metadata strings, and
`devBuyMinTokens` are not token constructor arguments. Salt is independently
bound by CREATE2; pool-only and launch-policy fields affect launch behavior or
state, not token/pool address identity. The full outer launch ABI is still
decoded canonically and bounded before prediction.

Primary references:

- [Bow official developer guide](https://bow.fun/docs.html)
- [Blockscout factory contract endpoint](https://robinhoodchain.blockscout.com/api/v2/smart-contracts/0xc70e510e14710ea535cab7b2414860af63feab79)

## Captured-chain reproduction

The implementation uses the captured `paper-session-20260716-long2` raw and
receipt evidence, not only the sample report.

| transaction | creator | salt | init hash | token | pool |
|---|---|---|---|---|---|
| `0x6ee43f064d3be9b696594913edef9252452424cf865823dc4e326e333f278458` | `0x88e4810d48a65ff7274df7829ef91e930a5eaf9c` | `0x...5306` | `0x23915dfdd1e0d5fae4a0b9834459c48a5c18932c0615826bb13695b354482b25` | `0xeff282419233a829d29dcf06230132bf55c6db03` | `0x5bf37a93a728f8ebd8c8d2288a1642ee2f8a6bcd` |
| `0xf842590f4b6abcda2e838397ceca82f07566d4256137a2ea88549cf331d1ab6b` | `0xd27664a94b801e912ef2051646f29ce76a8a3fb9` | `0x...4bc2` | `0xcb724966615630ef34838665e7badc5996013b80ab4f1b13dc940b3952d703ef` | `0xcede14a428b954333ba0e9a6df68d0e6fd786b03` | `0xeffe014849fb7056fd5aedd923e6dc0777d850ad` |

For both transactions, the locally reconstructed init hash equals the
factory's pure on-chain selector result, CREATE2 equals the exact `Launched`
event token, and the canonical V3 derivation equals the exact receipt pool.

## Fail-closed implementation

- The 7,841 creation bytes are a compile-time artifact; their length and hash
  are checked when the adapter starts.
- Startup now requires the exact Bow factory runtime in addition to the
  shared V3 pins. A drifted or missing factory fails adapter construction.
- Candidate prediction performs only canonical ABI encoding and local hashes;
  there is no RPC, filesystem, signer, or broadcast capability.
- Missing creator, malformed calldata, wrong chain/factory runtime, forged
  salt/creator/name/supply/max-wallet/limit-window, missing receipt identity,
  and forged receipt token or pool are covered by negative tests.
- Reconciliation accepts identities only after the caller has independently
  established exact factory-event receipt provenance.
- `execution_ready` remains false and restriction state remains unknown.

In repeated unoptimized local test builds, 100 complete predictions took
35,947 to 58,288 microseconds (359 to 582 microseconds per prediction). This is bounded and acceptable
for paper observation; it is not evidence for live execution promotion.
