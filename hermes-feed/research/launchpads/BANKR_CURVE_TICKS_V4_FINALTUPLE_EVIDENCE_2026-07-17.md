# Bankr CurveTicksV4 final-tuple evidence (2026-07-17)

## Scope and conclusion

This is a paper-only admission profile derived from the completed final-tuple
campaign. The authoritative reconciliation rows contain 16 successful Bankr
ground-truth launches: 9 in Window A, 4 in Window B, and 3 in Window C. All 16
decode to the same exact new curve profile:

- curve 0: ticks `[-229400, -119200]`, one position, 99% share;
- curve 1: ticks `[-119200, 887200]`, one position, 1% share;
- receipt liquidity, for the observed token ordering: `[119200, 229400]` with
  salt zero, then `[-887200, 119200]` with salt one;
- initialize tick `229400`, exact derived square-root price, and two nonzero
  liquidity additions from the pinned initializer.

This is `CurveTicksV4`. It does not relax or replace V1, V2, or V3. In
particular, their `-119800` and `-119400` joints remain distinct.

All 16 independently predicted token addresses are greater than WETH. V4 is
therefore admitted only for `token > WETH`; `token < WETH` fails closed in
pre-receipt observation/prediction, receipt quoting, and finalizer rederivation.
No inverse-orientation V4 profile is inferred or synthesized. V1-V3 retain
their existing two-orientation behavior.

The curve profile and outer envelope are orthogonal. Fifteen transactions use
the exact EntryPoint v0.7 `handleOps` / ERC-7579 path. Window B transaction
`0x5fac8d...7558e` uses the already-reviewed direct-Airlock path. Both paths
still require receipt-block proof of the same EIP-7702 account designator and
Kernel implementation/runtime. Direct Airlock requires no UserOperationEvent;
ERC-7579 requires exactly one successful, leader-bound, zero-paymaster event.

## Authoritative transactions

| Window | Transaction | Block / index | Envelope |
|---|---|---:|---|
| A | `0x05b0ffeb93614eedee2f18b9309fa0dd6aad155cc91f1c200bc32b39561cba55` | 11923737 / 1 | ERC-7579 |
| A | `0x0d7a7e2491ce085bb08b9cd97c8b492b681ece57a9772dc217c96de8ba91ec05` | 11922848 / 1 | ERC-7579 |
| A | `0x9e407b75206c95b3522b35b12d33c7aa4560dd0930032e042538d7ed34b9d716` | 11924153 / 3 | ERC-7579 |
| A | `0x74aae30e530ed4924e2ef6a20066bbad5011c34825bd2e929f98285ce29da3d6` | 11922728 / 3 | ERC-7579 |
| A | `0x7e8166aa043c107c1aeed96d4408906ba93d3a6f3790dcc862d2e1feecf7a537` | 11922749 / 2 | ERC-7579 |
| A | `0x5d8960558ca86480db79dea128351857c021e1aa5f3b3b7018ea5d60fbdfb4a3` | 11923622 / 7 | ERC-7579 |
| A | `0x29794f021ebe8922aeef97721e417352ebdb321b2cd210e779db298d42536953` | 11924130 / 2 | ERC-7579 |
| A | `0x920c52584343fd91c1034869221ba78b162034ce7f0108dace0347bde8cc3992` | 11922898 / 2 | ERC-7579 |
| A | `0x518247e23c9dca90b620483018fa8beae0ce899749c30bbaf245875791cb1da9` | 11922795 / 3 | ERC-7579 |
| B | `0x5fac8d13713912a64bb8ae17563e79d0c162e89a3eb8f5d12b3324d3c9b7558e` | 11926027 / 1 | direct Airlock |
| B | `0x9643945aaca673930fbdcad499529510553ae39976475b957da45fde730ea769` | 11926117 / 1 | ERC-7579 |
| B | `0xe57b3cf5738710edc4be8c7c1681c160b0a2fe71697e2c5bf79ea24490044eb1` | 11925746 / 1 | ERC-7579 |
| B | `0x85d67f7304c1418bc771127a18136aed45a05d057685622fd201594e095523bd` | 11928113 / 1 | ERC-7579 |
| C | `0x85d4ae7a0783bda7be2258762df36b11c78c04bf6d3cbe03504d52da1cd324c4` | 11929635 / 2 | ERC-7579 |
| C | `0x56d9d3c0c8ce10a4d664db4156eeaa2e17a69607680935b64157ff1abde855b6` | 11931270 / 2 | ERC-7579 |
| C | `0x0b6deb541255dee17afcec71ab874aba483dea037aea43240f6229883517705b` | 11931002 / 2 | ERC-7579 |

## Independent fixed-field proof

The committed fixture stores the complete transaction inputs, blocks,
receipts, receipt logs, receipt-block account designators, and one shared
receipt-block Kernel runtime. Tests independently decode every transaction and
apply the existing strict Bankr validator. Therefore all 16 must satisfy the
same existing pins and fixed fields, including:

- exact EntryPoint `0x0000000071727de22e5e9d8baf0edac6f37da032`,
  selector `0x765e827f`, one operation, empty init code and paymaster, and exact
  ERC-7579 selector `0xe9ae5c53` with all-zero mode for the 15 ERC-7579 calls;
- exact direct target/value/create selector for the one direct call;
- exact Airlock, WETH, token factory, governance factory, initializer,
  migrator, Rehype hook, integrator, supply/sell quantities, fee schedule,
  beneficiary weights and identity binding, token vesting shape, and Rehype
  routing fields;
- the 23-byte `0xef0100 || Kernel` designator with hash
  `0x4542cbf1da24ba964614e4f5585736e22884e23e97c0f0915de4f602585d2dd4`;
- Kernel runtime length 24,469 and hash
  `0x6f6d6691dc11fda98d3102802f20e7e816ccc576c16c9279ee1a884a51d1935d`.

No generic selector, arbitrary account, or alternative factory admission is
introduced.

## Tamper-evident provenance

- Window A `reconciliation-evidence.jsonl` SHA-256:
  `c6422a5983b65ab7f0b29a3c212360a88e968a02e7ccfd63d06b7eefe7132685`
- Window B: `e15ed22eb4897097899275cc8eadad04ddb85c3f0c60f18ff24f56afd00f5ab1`
- Window C: `3b41870e4668f30a11c7cf672be057f4ae4117458ca12825dba6ea6abb8812de`
- `bankr-doppler-v4-finaltuple-window-abc-live-proofs.json` SHA-256:
  `df98a39bb2f9ce948cce078d7f0dbf4e8dbf864919fd7618734b653e92e3e22c`
- independently generated V4 paper quote fixture SHA-256:
  `d366e76a11266e414811afb18f48ca347aaf92b0be0b18b23a4c17550381deab`
- independently generated direct-Airlock V4 paper quote fixture SHA-256:
  `be8ea9d2fc6e94f33bb37b283fb8cb17769eb410384607166b1a51d645e382b0`
- bounded raw-frame fixture SHA-256:
  `0d3322d880ceb77f63669470a1c76d28ab8cf2c0f7065b78383e235558c6c1fe`
- read-only receipt-block runtime-code fixture SHA-256:
  `d20f58d377cc13d12968659f59e3383cd78fac6a06824574af916c5f440dd0a6`.
  It records the exact nonempty Airlock, PoolManager, initializer, Rehype,
  token factory, token implementation, governance factory, liquidity
  migrator, WETH, EntryPoint v0.7, and delegated Kernel bytecode returned by
  `eth_getCode` at both proof receipt blocks. Every recorded Keccak-256 equals
  the independently reviewed production pin.
- Window A ERC7579 raw payload is exact `window-a/raw-feed.jsonl:2762`,
  received at `1784271655711031000`, payload-line SHA-256
  `4672011994f731bc6ca47ac8538c00539eb02c64854f8facbff1e2fff7291e75`.
- Window B direct-Airlock raw payload is exact `window-b/raw-feed.jsonl:1661`,
  received at `1784271886078187000`, payload-line SHA-256
  `2da502bfbc533b2188390ef7190c8f5316fb8084914f4cf821a83578d1c66a84`.

The capture used only read-only public JSON-RPC calls. No key, signer, wallet,
broadcast, execution flag, canary, remote host access, or deployment was
involved.

The concrete collector-dispatch test starts a loopback HTTP/1 JSON-RPC server
with the exact ordered receipt, canonical block, transaction, receipt-block
code, and final stability responses. Both raw Nitro frames cross
`PaperFeedRuntime`, serialized reconciliation-request parsing, and the
production `reconcile_candidate` Bankr branch before their results are compared
for exact equality with the checked-in ERC-7579 and direct-Airlock paper
quotes. A dependency-code mutation through that same branch must return a
blocked quote and consume no signing, execution, or broadcast path.

## Safety boundary

Observer admission and pre-receipt identity prediction use no candidate-time
RPC. Receipt-block identity and dependency proofs happen in reconciliation.
Every reconstructed quote remains `execution_eligible = false` and
`broadcast = false`; readiness/finalization also preserve
`candidate_time_rpc = false` and execution-gated output.
