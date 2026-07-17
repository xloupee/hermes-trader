# Bankr CurveTicksV5 fresh evidence (2026-07-17)

## Bounded conclusion

Six successful Bankr ground-truth launches from the fresh immutable three-window
paper campaign share one exact successor curve profile:

- calldata curve 0: `[-229200, -119200]`, one position, 99% share;
- calldata curve 1: `[-119200, 887200]`, one position, 1% share;
- receipt position 0: `[119200, 229200]`, salt zero;
- receipt position 1: `[-887200, 119200]`, salt one;
- initialize tick `229200` and exact `sqrtPriceX96`
  `7510096409285047843309134522194364`; and
- predicted token greater than WETH in every proof.

This profile is named `CurveTicksV5`. It is an exact additional profile; V1-V4
remain unchanged. All six proofs use the reviewed EntryPoint v0.7 / ERC-7579
single-call envelope. V5 therefore admits only that envelope. A direct-Airlock
V5 call fails closed at observer admission, receipt quote admission, and
finalizer replay. No inverse token orientation or direct envelope is inferred.

## Exact transactions

| Window | Transaction | L2 block / index | Receipt-block leader |
|---|---|---:|---|
| A | `0x4c910a52338472b365dadec2dd0bd24443f189396674750d74e93226e8e36fd6` | 11991651 / 2 | `0x14d1aba5250281397874080b52f7e8ad2b04f48e` |
| B | `0xd1641af3d4bfc5edb4efc118d0e3de7370c3d7385c5be74d138e2317485a582b` | 11994560 / 1 | `0x14d1aba5250281397874080b52f7e8ad2b04f48e` |
| C | `0xc62d71574bc598c026a8f44d4aacac2a599fb61edbb41a117d0ed19e2e1c8a51` | 11995775 / 4 | `0x83373304901343625c41ac27bda93e5bb22f8e7c` |
| C | `0xd513a87cc85a141ef88fb3ca7a0ce618223930a2585ff7ca9d0400ff2a2eee9f` | 11996139 / 2 | `0x47433e5734d403ddf07530a9ff131a6a51fc5c8e` |
| C | `0x0ac9b544c6f5e345bbfc4697eca18f185592f4fb935583829160995cf919e9d6` | 11996520 / 5 | `0x14d1aba5250281397874080b52f7e8ad2b04f48e` |
| C | `0x227b398f6f2e4d61ba30c4d32a163d359dae1e073f4fcbbd94c1641117526ebc` | 11998586 / 1 | `0x4e86ed3ca14ae871d3a00505453608fd4c498baa` |

Every transaction targets EntryPoint `0x0000000071727de22e5e9d8baf0edac6f37da032`
with `handleOps` selector `0x765e827f`. Strict unwrapping requires the existing
per-account ERC-7579 execution profile, selector `0xe9ae5c53`, all-zero mode,
Airlock target `0xeb7c034704ef8dcd2d32324c1545f62fb4ad0862`, zero value, and inner create
selector `0x882db707`. There is no global selector dispatch.

At each receipt block, the leader code is the 23-byte
`0xef0100 || 0xd6cedde84be40893d153be9d467cd6ad37875b28` designator with hash
`0x4542cbf1da24ba964614e4f5585736e22884e23e97c0f0915de4f602585d2dd4`.
The delegated Kernel runtime hash remains
`0x6f6d6691dc11fda98d3102802f20e7e816ccc576c16c9279ee1a884a51d1935d`.
All Airlock, PoolManager, initializer, Rehype, factory, implementation,
migrator, WETH, and EntryPoint production pins are unchanged.

## Deterministic artifacts

- six transaction/receipt/block/designator proofs SHA-256:
  `204c2d8ccc4e46cfd1a00185111a8215cb0dbb0b2d6402a36c649386b5238dec`;
- exact raw Nitro frame fixture SHA-256:
  `dfa0c3c848940c5913c0391857bf1ef7ff17986f9fd1aa957ab0ef13c17457a7`;
- raw payload plus newline SHA-256:
  `8c17197e9de53e4a65288729d0daf0c08a1489dde12ddb76350c8628dc6988b3`;
- independently generated paper quote SHA-256:
  `72c598aea39e3829b805a2d99c62b4c4b99f01462a2280cc059b1544d0cf6e23`;
- exact receipt-block dependency bytecode fixture SHA-256:
  `a2410ef64a98f8856cf0d3f9721c66403a4290cddd6348290af5ddf91ed4565e`.

The raw frame is `window-a/raw-feed.jsonl:3100`, received at
`1784278482394028000`. It crosses `PaperFeedRuntime`, the serialized strict
reconciliation-request parser, the production `reconcile_candidate` dispatch,
and a concrete loopback `NoxaRpcClient` JSON-RPC transcript before exact quote
comparison. EntryPoint or dependency runtime drift returns a blocked quote.
The paper finalizer independently rederives orientation, tick, square-root
price, position ranges, salt ordering, and quote arithmetic.

## Safety and limitation

All collection used read-only public feed/RPC evidence. No wallet, key,
keystore, signer, signing, broadcast, execution, canary, deployment, Droplet,
or server access was used. Every quote and finalized plan remains
`execution_eligible = false` and `broadcast = false`; candidate-time RPC remains
disabled.

The six RPC proof records preserve exact transaction input/from/to/value and
receipt/block identity but do not store signed raw transaction bytes. One exact
captured Nitro frame independently proves the complete signed feed envelope for
the canonical V5 case. This bounded result does not authorize a direct V5
envelope, inverse orientation, or any live canary.
