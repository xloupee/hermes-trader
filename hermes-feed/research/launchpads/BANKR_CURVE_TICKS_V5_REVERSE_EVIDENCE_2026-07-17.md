# Bankr CurveTicksV5 reverse-orientation evidence (2026-07-17)

This is a paper-only, fail-closed extension of the existing exact
`CurveTicksV5` profile. It admits `token < WETH` only through the already
pinned EntryPoint v0.7 / ERC-7579 / EIP-7702 Kernel envelope. It does not add
signing, execution, broadcast, canary, or direct-Airlock authority.

## Source boundary

- audited code: `08546d508d2c379e495960f8dc1425d4b6c44e9e`
- frozen campaign root:
  `hermes-feed/.runtime/post-bankr-pons-08546d5-20260717T142717Z`
- exact miss rows: `campaign-3w/windows/window-a/reconciliation-evidence.jsonl`
- raw Nitro frames: `campaign-3w/windows/window-a/raw-feed.jsonl`
- public read-only RPC: `https://rpc.mainnet.chain.robinhood.com`

The timestamped root named `post-bankr-pons-08546d5-20260717T181649Z` was not
present in the local worktree. The `...T142717Z` root is the complete six-window
campaign root at the requested exact code and contains all four reported Bankr
misses.

## Verified repeat proof

All three transactions below are successful EntryPoint v0.7 calls with outer
selector `0x765e827f`. Each contains one account call with selector `0xe9ae5c53`,
all-zero ERC-7579 mode, target Airlock
`0xeb7c034704ef8dcd2d32324c1545f62fb4ad0862`, value zero, and inner selector
`0x882db707`.

| transaction | L2 block / index | outer bundler | leader | token | pool id |
|---|---:|---|---|---|---|
| `0xf05362bfc3dd65c67116b1630e8872e80380d2f6f7561455f4bbea9b2dcb391a` | `12192219 / 5` | `0x7540c25c054f6cf509862793239630be4e83af8d` | `0x45a52b682617bfe091138b8aa9926a608c692143` | `0x08659aef179de34ba122c170af932ebe0d209ba3` | `0x0ccdb9dc3ca3c2e9a5d3420ef8d6335544588e904f91372e86361acc8351cc42` |
| `0x7c3641c37918052cf50e323ab99d99cd539ddd96c5c8f13511cc23db4ea8cd18` | `12192818 / 5` | `0x17a9aa3f7945fb00c2eb16857baa4adb63da59db` | `0x06b7e0519639f9322930d47992464aa7c4784c86` | `0x0b20298a0807b5cb4e29a59a09f754dfcbebdba3` | `0x18f2576d8f3002f0997d8cabe0a7964b442f0ea5c20c440ab84d13cdb81e2879` |
| `0x81a8eb424f298df231b6d7e5acf8fafb7816742b80bd2b9b71caf8292b1c8bfc` | `12193370 / 12` | `0x463928d45ed7e052b62ee86999c6d75f861e1944` | `0xbe73c058ed90983187d3e39fced6fe379210408b` | `0x0b7c35adcc52ca404dc471811c9878f1ca858ba3` | `0x2bb56a0a01ce1a2c462bfbf24a73c0ddfccb9304ab31ade33ad601e49ca91fd9` |

For all three, the token is strictly below WETH
`0x0bd7d308f8e1639fab988df18a8011f41eacad73`. The create calldata is exact
`CurveTicksV5`:

- curve 0: `[-229200, -119200]`, one position, 99%;
- curve 1: `[-119200, 887200]`, one position, 1%;
- LP fee `7000`, tick spacing `200`, far tick `887000`;
- calldata salts, respectively:
  `0x05cdeda696cd6676e4bb8235d40751b76725b1afa5f43ee58af8860db61407e3`,
  `0x101d0ea951787eefcb8052b01a1d3eb87ad426aab9dc8067b1de853e05fe9b5b`,
  and `0x4afa9b914999ab318e5b8f63bc1dadfd98eb6b12b1bb43770698a8f1e0092969`.

The three receipts independently repeat the same orientation-specific state:

- initialize tick `-229200`;
- initialize sqrt price X96 `0xb0fdfc8b493ef2e496b6`;
- position 0 `[-229200,-119200]`, liquidity
  `0xbcc248d856f01dd554c5`, salt zero;
- position 1 `[-119200,887200]`, liquidity
  `0x1d082240a370451eb5ea2`, salt one;
- no matching-pool swap before the first eligible paper quote;
- Airlock launch followed by a successful UserOperation event.

## Receipt-block pins

Each leader has exact EIP-7702 designator
`0xef0100d6cedde84be40893d153be9d467cd6ad37875b28`, hash
`0x4542cbf1da24ba964614e4f5585736e22884e23e97c0f0915de4f602585d2dd4`.
The delegated Kernel runtime at
`0xd6cedde84be40893d153be9d467cd6ad37875b28` is 24,469 bytes with hash
`0x6f6d6691dc11fda98d3102802f20e7e816ccc576c16c9279ee1a884a51d1935d`.

The exact receipt-block dependency hashes remain the independently reviewed
production values; no observed value is promoted here:

- Airlock `0x86b37100cbe9841771c452a592985b4e921254b127a380246073b84ec953f7f8`
- PoolManager `0xbd3881180b547f5fe817545743cfb4343e96b1bc6640dcd70c106b0066e95626`
- initializer `0xc41a91106002f15bf70ae266824317f3f3ac638ac72ca5253bae395fa47ee631`
- Rehype hook `0x5d33a1d867ba0d17cc7af077786b1356848c72f8e0bf960ef88aa15f7a6962d1`
- token factory `0x27abd63146eb5743b7871e211da17163afbb495863a626c0d002312af6813459`
- token implementation `0x67a382a66d2b14a7032698e11c9ae4432435d2c803429d5c660692289ad10e12`
- governance factory `0xefce8ac4a6fe83ae3dd1c3cfebc0e370e1595a66608bed5610ffdd1f291b7f63`
- liquidity migrator `0x7bf5115543e8e0769ceabe4da9b8e23547c9e95c1cce15d24d96f164406129e3`
- WETH `0x5706be52f64875fee65a2cec0d80e47a23d8793cbe85d214b48445e2d05f5353`
- EntryPoint v0.7 `0x8db5ff695839d655407cc8490bb7a5d82337a86a6b39c3f0258aa6c3b582fc58`

## Separately unsupported fourth miss

`0x0ac1e64aed41a544bc3f5b923b6f4330628950c917674a8e0cf1348d1944c0c0`
at L2 block `12212166`, index `5`, is not reverse V5. It is a direct Airlock
transaction from code-empty EOA
`0x594fa0407908732f1f9269bf0cc86af662473bf3`, with token
`0x5a2b1eb2dd3bbeddd8ba0a22b23477c87cc3b154` above WETH. Its distinct create
shape is LP fee `100000`, curves `[-228600,-118600]` at 99% and
`[-118600,887200]` at 1%. Its receipt initializes at tick `228600`, sqrt price
X96 `0x16755643048d7f68fafbf0c918fea`, then adds `[118600,228600]` liquidity
`0xc281d7af71a57d38d932`/salt zero and `[-887200,118600]` liquidity
`0x1dea76e85b6878e8d4f29`/salt one. It remains unsupported and has a dedicated
negative fixture.

## Classification

Verified: the first three transactions are repeated, exact reverse-orientation
CurveTicksV5 ERC-7579 proofs with identical receipt state and existing pins.

Inference/policy: three independent transactions are sufficient to add this
single bounded paper profile while retaining the normal per-launch salt and
creator fields already accepted for forward V5.

Unknown: the economic intent and future stability of the distinct fee-100000
direct profile. It is not admitted by this change.
