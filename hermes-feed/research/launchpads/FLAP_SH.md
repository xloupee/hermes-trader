# Flap.sh on Robinhood Chain (chain 4663)

Research snapshot: 2026-07-16 04:47 UTC

Hermes baseline: `5c1827ec57a155e24c30bb32204ffa76352991ea`

Decision: **Tier 2, observe-first**

## Executive conclusion

Flap officially supports Robinhood Chain. This is not merely a generic Dune
label or deployer attribution:

- Flap's first-party production route, [`flap.sh/robinhood/board`](https://flap.sh/robinhood/board),
  identifies `robinhood`, chain ID `4663`, and embeds a Robinhood-specific
  Portal, VaultPortal, token implementations, backend, websocket, WETH, curve,
  and fee configuration.
- The route uses Flap's official domain, branding, docs, and social links.
- A public mirror of an `@flapdotsh` announcement says Flap infrastructure,
  Tax Token V3, and Stocks Vault are available on Robinhood Chain. The mirror
  is secondary evidence because X was not directly accessible in this pass.
- The chain contracts are live, verified where noted, and emit canonical Flap
  events under sustained activity.

The apparent conflict comes from documentation drift: Flap's official
[`Deployed Contract Addresses`](https://docs.flap.sh/flap/developers/deployed-contract-addresses)
page currently lists BNB, Toshimart, X Layer, Muffun, Monad, and BNB testnet,
but omits Robinhood. The production app and chain deployment are newer than
that table. A Dune label may still be broad or double-counted, but it is not
the basis for concluding that Flap supports Robinhood.

Tier 1 is not justified yet. Portal and VaultPortal are upgradeable; the
deployment supports multiple token versions, curve state, taxes, extensions,
quotes, migrators, and DEX paths; the VaultPortal implementation is not source
verified; and the bounded scan did not resolve the complete graduation and
post-migration routing surface. Hermes should first add a pinned, fail-closed
observer and admit only a narrow, explicitly allowlisted simple profile.

## Evidence labels

- **Verified** means observed in an official Flap/Robinhood source, verified
  Blockscout source/ABI, or a public RPC/Blockscout response during this pass.
- **Inference** means a conclusion derived from verified configuration or
  behavior but not stated directly by the protocol.
- **Unknown** means the bounded pass did not establish the fact. Unknowns are
  not treated as zero.

## Network and deployment identity

**Verified.** Robinhood's official [connection documentation](https://docs.robinhood.com/chain/connecting/)
specifies mainnet chain ID `4663`, ETH as the native gas asset, the public RPC
`https://rpc.mainnet.chain.robinhood.com`, and Blockscout at
`https://robinhoodchain.blockscout.com`. The public RPC returned chain ID
`4663` at 2026-07-16 04:46 UTC.

### Primary contracts

Runtime hashes are `keccak256(eth_getCode(address, latest))` from the public
chain snapshot.

| Role | Address | Identity / runtime hash | Status |
|---|---|---|---|
| Portal proxy | `0x26605f322f7fF986f381bB9A6e3f5DAb0bEaEb09` | `0xcecb292d9c022858199c9348abf0d5836f9ea4dab5cf03710e1dcf41fd9a4c35` | Verified EIP-1967 `TransparentUpgradeableProxy` |
| Portal implementation | `0xd9C9981D784A3765D8264D6104650B901C4e36b1` | `0x85facd83c203c88ea8f37c4f00c328f983e90c5045b06ec20ef18639c818186b` | Verified `Portal`, Solidity 0.8.26; live `version()` = `v5.14.16` |
| VaultPortal proxy | `0xe9F7AB7DE8FB8756acbB6a1cd13316a43308197B` | `0xe7109718479fd7c6d05b829ffc6a1469e4c949ae282497c15d179b2af4e5e3a9` | Verified EIP-1967 proxy; also returned by `Portal.VAULT_PORTAL()` |
| VaultPortal implementation | `0x2813CD0b6089f76F3407792f79276E5d4f80935A` | `0x4f096b230a8db270585d54fdd549982efda99462daad9c4b3e771a62e7071f56` | Implementation address/hash verified; source is not verified |
| Standard token implementation | `0x88882688a067FE97E11C2185b996286e53132222` | `0x40c79e4f08bc0f8da02b1314b2a7987111b6898adcdcef691579dda6f2fcabf2` | Verified `FlapNonTaxToken` |
| Tax Token V3 implementation | `0x7777C8743C88B3aff3cf262135beF2c8b2e83333` | `0xa73abf611d52de6364ec684feed2ef3e9aec9706a02b808523e75a6d8438b164` | Verified `FlapTaxTokenV3` |
| Tax helper proxy | `0xb10bD2672aE63735d677164A54B573a016f0203C` | Not pinned in this bounded pass | First-party app configuration |
| SwapRegistry proxy | `0x35Bae0b77753a586f68f9C4CD0E8d1a468169031` | Not pinned in this bounded pass | First-party app configuration |
| WETH | `0x0Bd7D308f8E1639FAb988df18A8011f41EAcAD73` | Existing Hermes Robinhood constant; hash not re-pinned here | First-party app and chain configuration |
| IndexVault factory | `0xe6ca297D1d963b6F00d5b216986123CAeB883AF6` | `0x37ad8d77398199bf43ec8b2cf20065264d96c3b540dd198c64a8a70d1537fe7f` | Only vault type exposed by the current Robinhood UI |

Portal was created by `0x3bfC05a8b9e48FdFd6A443657caC5D983B664a05`
in transaction
[`0x66ab…cdbc`](https://robinhoodchain.blockscout.com/tx/0x66ab432afa53aca015e57d94b4c0057d02e5a02600343dff6e66e6ea1281cdbc),
block `4,180,724`, at `2026-07-08T07:34:50Z`. VaultPortal was created by
the same address in transaction
[`0x9b34…af6c`](https://robinhoodchain.blockscout.com/tx/0x9b347fe0179ba6e4d61a0633dee6324dc2eee9ab61df312fc47c55e1a758af6c),
block `4,224,247`, at `2026-07-08T08:47:33Z`.

### Portal modules, migrators, and DEX routing

**Verified.** These addresses are immutable constructor parameters of the
verified Portal implementation.

| Role | Address | Runtime hash |
|---|---|---|
| Token launcher | `0x662575ba5540af30531b1f1acb852c81e2ada2a9` | `0x884973b6221563b3efd4afb79cac370d7391809d9b31ddcb6b8e1506d7b897ab` |
| Token trade V2 | `0x362fd190fa57ea181b85c86df2e5b2113c2834c7` | `0x00ddde00576929b0f09ad72632669fe97f7ed2ceb73e7701e13df95db3b5efb2` |
| Portal DEX router | `0x6f8de19af17c3622af9342930f8f459e429f31f7` | `0xf3491fbf71c566a33130b14a688b67ea3e5d7b4b0c0ac6046e87dd8f0fa45814` |
| Uniswap V3 migrator | `0xa80cc552c2b425715d73dbb3f71e754788377dd4` | `0x35641decfa2b70b956a84a964ea47864c74f73755fd5d25bef0d4cfa3a62c242` |
| Uniswap V2 migrator | `0x40373043d4a672c55f1dde0ae137e9da4ab37083` | `0xaee87486e47cec3d1f7a125e84e975ae2d5df31610e955ab34d909173220d6ca` |
| Uniswap V4 migrator | `0x1c8847736521f5cd725dfb8f33c7c610826e7c42` | `0x32fd5bc20ab94f5774a35023621a79ac996603b364b3e43e3867fc8446f74eae` |
| Multi-DEX router | `0x12008835d60fcda9f6bc6ba8fb0f8e37539771eb` | `0x31f50be79a593a3de332e6c573428a715bf276fe81e7484e7683f163a705ae74` |
| V4 PoolManager | `0x8366a39cc670b4001a1121b8f6a443a643e40951` | Not pinned in this bounded pass |
| V4 PositionManager | `0x58daec3116aae6d93017baaea7749052e8a04fa7` | Not pinned in this bounded pass |
| Portal V4 CL locker | `0x468a2647c6b1ec4d4bc2b09e17ea9ebf9e7c915d` | Not pinned in this bounded pass |

**Verified configuration; inferred route.** The first-party Robinhood app sets
`legacyNonTaxMigratorType = 1`, `useLegacyNonTaxV5 = true`, and passes
`migratorType = 1` for its Tax V3 flow. Flap's enum defines `0` as V3 and `1`
as V2. The current UI therefore appears to select V2 migration for its normal
Robinhood launch flows even though V3/V4 modules are deployed. This is an
inference from current frontend configuration, not proof that every historical
token or non-UI caller used V2.

**Unknown.** This bounded pass did not independently pin the underlying V2/V3
factory and router selected by every DEX ID or prove that Hermes' existing
Robinhood Uniswap constants are the contracts used by every Flap migration.
Those must be read from the pinned router/migrator configuration before any
post-migration trading path is admitted.

## Launches and deterministic token addresses

### First-party launch entrypoints

**Verified.** The current Robinhood frontend uses:

- standard launch: `newTokenV5(...)`, selector `0x2e2fdbd9`;
- Tax V3 launch: `newTokenV6(...)`, selector `0x8cb5772c`;
- Vault-backed Tax V3: VaultPortal `newTokenV6WithVault(...)` (generate the
  selector from the pinned VaultPortal ABI; do not copy a handwritten tuple
  signature);
- staged/two-step V5 methods also exist in the Portal ABI.

The verified Portal ABI contains `newTokenV2` through `newTokenV7`. A detector
that filters only `newTokenV5` will miss valid launches and should be described
as a narrow profile, not universal Flap support.

### CREATE2 identity

**Verified.** Flap's first-party salt worker constructs an EIP-1167 minimal
proxy creation payload:

```text
0x3d602d80600a3d3981f3
  363d3d373d3d3d363d73
  <20-byte implementation>
  5af43d82803e903d91602b57fd5bf3
```

The Portal deploys it with CREATE2 and the user-supplied salt. The worker
searches salts for the configured vanity suffix (`8888` standard, `7777` tax).
Therefore token identity is predictable before execution when all of these are
pinned: Portal/deployer, token version and implementation, exact minimal-proxy
init code, and salt. `FlapTokenStaged` represents a two-step path whose address
is predetermined before creation.

Hermes must not predict with one global implementation hash: standard, taxed,
versioned, extension, and staged paths can select different init code.

## Canonical events and selectors

### Creation and enrichment

Flap's official [indexing guide](https://docs.flap.sh/flap/developers/wallet-and-terminal-and-bot-developers/index-token-created-events)
says to index `TokenCreated`, then enrich it using optional events in the same
transaction and apply version-correct defaults when an event is absent.

| Event | Topic 0 |
|---|---|
| `TokenCreated(uint256,address,uint256,address,string,string,string)` | `0x504e7f360b2e5fe33cbaaae4c593bc55305328341bf79009e43e0e3b7f699603` |
| `FlapTokenStaged(uint256,address,address)` | `0x2bf0a17cc6127084d945eb95a40df6c839234845722b025fdeef767b9464c02d` |
| `TokenCurveSet(address,address,uint256)` | `0xda7793e72bf76b338906a98bfb58526d202873ecaf7a5894663053ee69069ce1` |
| `TokenCurveSetV2(address,uint256,uint256,uint256)` | `0x71a10912a55f73d3cced0d1515c2b33c396c80342522bad0e295ccbede556f37` |
| `TokenDexSupplyThreshSet(address,uint256)` | `0x6f10bbe11587431707df676556a0551025a4f66551acdddc5793389e20f0d46e` |
| `TokenQuoteSet(address,address)` | `0x3ceb902d3c555c21c3415b6aa839104b18e4825b2f8324011ff979089a507a8c` |
| `TokenMigratorSet(address,uint8)` | `0xcf2372b9357f0d392563d2cedb11e2b3bf0c14d2b8b75eb8bad073bbea9b0ff9` |
| `TokenVersionSet(address,uint8)` | `0x37502bf23c59a12e1036e7580a8dc056803623dbcd0885abb882adfa069ac89e` |
| `FlapTokenTaxSet(address,uint256)` | `0x1546924f4680b1b2e093fd251c437c8781d81a21ed8ad1895e1f2c9b78db0cd0` |
| `FlapTokenAsymmetricTaxSet(address,uint256,uint256)` | `0x46cc246a238d1ca0951a15200994903e2d56cbb0389e63f09d66412a787aa3c0` |
| `TokenExtensionEnabled(address,bytes32,address,uint8)` | `0x325114fb533bfc4cffa883f4b9d31f82bf4d977d38112f80e382bbc7f5b39714` |
| `TokenDexPreferenceSet(address,uint8,uint8)` | `0x6b3125ac92c93333dab20ade84015b8cde845176b110057b49a81768c0cda374` |

All `TokenCreated` fields are unindexed on this deployment. Address plus topic
0 can find the log, but creator/token/name/symbol/meta require decoding `data`.

### Trading and graduation

| Method/event | Selector/topic 0 |
|---|---|
| `quoteExactInput((address,address,uint256))` | `0xfc847c2b` |
| `swapExactInput((address,address,uint256,uint256,bytes))` | `0xef7ec2e7` |
| `buy(address,address,uint256)` | `0x153e66e6` |
| `buyOnCreation(address,address,uint256)` | `0xfcb5c9e3` |
| `sell(address,uint256,uint256)` | `0x6a272462` |
| `getTokenV5(address)` | `0x5c4bc504` |
| `getTokenV6(address)` | `0xdbde08f0` |
| `TokenBought(uint256,address,address,uint256,uint256,uint256,uint256)` | `0xa800a2038683844fac66747f771bfdfae862eb28b16bcfa387afa9fbacce8ff7` |
| `TokenSold(uint256,address,address,uint256,uint256,uint256,uint256)` | `0x03a4693e592f5e75dc7c136acb39b146d2b4966c0e509c34f362dee02b3b861a` |
| `FlapTokenCirculatingSupplyChanged(address,uint256)` | `0x115c78ad17c4763fb97bca94f3e59dc8cb2e59c9d3862f24a694ec401200f562` |
| `LaunchedToDEX(address,address,uint256,uint256)` | `0x6e4f47630b8745b8cacbd44f42a8a33e7eea7cc08ef22fc7630f4f385784ff7d` |

Tuple selectors must be generated from the pinned ABI in implementation code.
The table records the verified snapshot, not a permanent compatibility promise.

## Curve, quote, tax, extension, and vault behavior

### Curve and quote

**Verified.** The current Robinhood UI exposes only native ETH as a payment
token and configures:

```text
r = 1.9189797
h = 107036752
k = 2124381054.2419343
dexSupplyThresh = 800,000,000 tokens in observed creation events
total supply = 1,000,000,000 tokens (18 decimals)
```

The curve is the Flap CDP V2 constant-product form documented in
[`Bonding Curve`](https://docs.flap.sh/flap/developers/basic-and-mechanism/bonding-curve):

```text
(1e9 + h - circulatingSupply) * (reserve + r) = k
```

Flap explicitly warns integrations not to hardcode chain-level curve values.
Hermes should cache immutable per-token `r`, `h`, `k`, threshold, quote token,
and decimals from same-transaction events plus an asynchronous pinned-block
`getTokenV9Safe` verification. The latest lens also exposes status, pool,
progress, fees, DEX ID, LP profile, extension ID, and tax fields.

### Buy and sell route

**Verified.** On the bonding curve:

- native buy: `inputToken = address(0)`, `outputToken = launched token`, and
  `msg.value` supplies the exact input;
- sell: `inputToken = launched token`, `outputToken = address(0)` for the
  current native-quote profile;
- ERC-20 quote and native-to-quote conversion paths exist in the general ABI;
- `quoteExactInput` is not `view`, but is intended for `eth_call` simulation;
- `swapExactInput` is the current exact-input route, with minimum output and
  optional permit data.

Post-migration, the Portal can delegate to `PortalDexRouter`, but a trader can
also interact with the migrated DEX directly. Portal-only transaction filters
are therefore incomplete after graduation.

### Taxes and extensions

**Verified.** The UI sets 1% buy and sell protocol fees. Flap's
[`PreBond Tax`](https://docs.flap.sh/flap/developers/basic-and-mechanism/flap-tax-token/prebond-tax)
documentation says pre-migration token tax is modeled as an added curve fee:
effective fee = protocol fee + token tax. After migration, taxed token
implementations can levy transfer taxes, accumulate tokens, and automatically
liquidate them through a tax processor. Tax V3 supports asymmetric buy/sell
rates and more allocation fields.

Extensions are opaque unless the extension ID, address, version, bytecode, and
behavior have been allowlisted. The current UI uses a zero extension by
default, but the chain ABI permits non-zero extensions. Unknown extensions are
observe-only.

### Vault risks

Portal launches standard/tax tokens. VaultPortal additionally predicts and
creates a vault around a tax token. The current Robinhood UI exposes only the
IndexVault factory, but Flap's vault specifications allow external vault
factories and custom behavior. Risks include:

- upgradeable VaultPortal logic and an unverified current implementation;
- arbitrary/unreviewed vault factories and adapters;
- privileged guardian/admin behavior;
- tax revenue redirection, liquidation, dividend, and external-call logic;
- a token address being predictable before the vault is created;
- operational dependencies such as trigger services.

Vault-backed launches should remain observe-only until the exact factory,
implementation, runtime hash, configuration, and permissions are allowlisted.

## Migration and graduation

Flap's official [migration documentation](https://docs.flap.sh/flap/developers/wallet-and-terminal-and-bot-developers/token-migration)
says reaching `dexSupplyThresh` creates a DEX pool and emits
`LaunchedToDEX(token, pool, amount, eth)`. The broader ABI also contains
`FlapTokenCLPoolCreated`, `TokenPoolInfoUpdated`, multiple migrator types, DEX
IDs, and LP fee profiles. Tax tokens may follow different migration rules from
standard tokens.

The strict Portal `LaunchedToDEX` topic scan returned zero events in both exact
activity windows below. **This is not reported as zero graduations.** Current
Portal `v5.14.16` has alternative CL-pool and external-migrator event paths,
and the bounded pass did not complete a cross-contract migration scan.

Consequently:

- verified 24h graduations: **unknown**;
- verified 7d graduations: **unknown**;
- migrated liquidity and current pool liquidity: **unknown**;
- canonical pool identity per DEX/migrator profile: **unknown until pinned**.

## Measured activity

These counts come from direct `eth_getLogs` scans against the canonical Portal
address and exact topic 0, in 50,000-block chunks. They do not use a Dune entity
label. The latest measurement block was `10,980,306`, timestamp
`2026-07-16T04:46:18Z`.

| Metric | 24h: blocks `10,115,167..10,980,306`, `2026-07-15T04:46:18Z..2026-07-16T04:46:18Z` | 7d: blocks `4,941,282..10,980,306`, `2026-07-09T04:46:18Z..2026-07-16T04:46:18Z` |
|---|---:|---:|
| `TokenCreated` launches / unique tokens | 10,852 | 31,697 |
| Unique `TokenCreated.creator` values | 1,499 | 6,134 |
| `TokenBought` events | 35,479 | 141,231 |
| Unique event buyers | 2,632 | 6,469 |
| Raw buy event field named `eth` | 852.420969719242526906e18 units | 3921.955374139991743322e18 units |
| Raw buy fee field | 8.524209697192423427e18 units | 39.219553741399911515e18 units |
| `TokenSold` events | 30,458 | 115,630 |
| Unique event sellers | 1,644 | 4,057 |
| Raw sell event field named `eth` | 982.454865743392985135e18 units | 4014.690116653328122936e18 units |
| Raw sell fee field | 10.056743589155814112e18 units | 41.254762821257575302e18 units |
| Total buy + sell events | 65,937 | 256,861 |
| Normalized economic volume | **Unknown** | **Unknown** |
| Graduations | **Unknown** | **Unknown** |
| Migrated/current liquidity | **Unknown** | **Unknown** |

The ABI names the quote-side trade field `eth`, but Portal supports configurable
quote tokens and native-to-quote conversion. This pass did not enumerate and
normalize each traded token's quote address and decimals. The raw sums above
are reproducible event-field aggregates; they are not claimed as ETH or USD
volume.

As a consistency check, the exact UTC calendar day 2026-07-14 (blocks
`9,081,446..9,943,952`) contained 11,555 `TokenCreated` events/unique tokens
from 1,811 event creators, 26,707 buys, and 16,230 sells. A public claim of
roughly 22,000 Flap tokens on July 14 is not reproduced by canonical Portal
`TokenCreated` under this UTC-day definition. Possible timezone, rolling-window,
double-counting, or broader-deployer-label explanations remain inference until
the underlying query is inspected.

## Feed visibility

### Pre-receipt signal

**Verified.** Direct calls to Portal and VaultPortal are ordinary signed EVM
transactions and are visible in Robinhood's Nitro sequencer feed transaction
bodies. Hermes can filter `to` plus a small selector set before doing full ABI
decode. Standard/tax launch calldata contains the salt and profile fields needed
for deterministic work when the selected implementation is already pinned.

### Post-execution proof

`TokenCreated`, optional enrichment events, `TokenBought`, `TokenSold`, and
migration events are receipt logs. They do not exist in the pre-execution feed
transaction body. They are authoritative asynchronous proof and registry input,
not a pre-submit dependency.

Coverage caveats:

- launch methods span multiple Portal versions plus VaultPortal and staged
  flows;
- the same transaction can emit several required/optional state events;
- after migration, swaps can go directly to DEX contracts;
- aggregators can call Portal or DEX routes on behalf of the user;
- token/creator/trader fields in the principal Portal logs are unindexed;
- proxy upgrades can change accepted selectors and semantics without changing
  the Portal address.

## Hermes integration seams

The current Hermes design already has the right primitives, but the Noxa path
must not be reused by changing constants. Flap is a multi-version bonding-curve
protocol with a different transaction and state model.

### Tier 2 implementation shape

1. **New isolated module family**
   - `flap_abi.rs`: pinned calls/events, strict tuple decoders, selector/topic
     constants, no metadata/IPFS parsing on the hot path.
   - `flap_candidate.rs`: cheap `to` + selector filter, then strict full decode.
   - `flap_registry.rs`: immutable per-token snapshot keyed by token address and
     profile; updated only from verified receipts and pinned-block lens reads.
   - `flap_verifier.rs`: Portal/VaultPortal proxy implementation, runtime hash,
     `version()`, module hashes, token implementation, and same-transaction event
     reconciliation.
   - `flap_curve.rs`: exact integer CDP V2 quote math with protocol/tax fees and
     quote decimals; differential tests against `quoteExactInput`.
   - `flap_observer` binary: no signer, no sender, no service coupling.

2. **Reuse existing generic seams**
   - `decoder.rs`: its transaction visitor already exposes feed sequence, L1
     block/timestamp, envelope, signer, destination, value, and calldata.
   - `boundary_gate.rs`: only after Flap-specific timing is measured; do not
     assume the Noxa `B + 1` restriction.
   - `paper_runtime.rs`: reuse risk/nonce/accounting after a Flap-specific quote
     and route plan exists.
   - `trading_runtime.rs`, `signer.rs`, `hot_path.rs`, and `sequencer.rs`: remain
     generic send machinery, but signed Flap use is out of scope for Tier 2.
   - `robinhood.rs`: add Flap constants only after pins are reviewed; do not
     mix them with Noxa identities.

3. **Startup preload and pinning**
   - Portal/VaultPortal proxy and implementation code hashes;
   - `version()` and accepted selector/topic manifest;
   - launch/trade/migrator/router modules and their code hashes;
   - token implementation init-code templates per version;
   - quote token configuration, fee configuration, curve defaults, DEX config,
     and extension/vault allowlists.

4. **Hot-path order**

```text
Nitro feed tx
  -> destination allowlist
  -> four-byte selector allowlist
  -> strict ABI decode
  -> profile/version/quote/extension/vault policy
  -> deterministic token prediction when fully pinned
  -> local exact quote and paper decision
  -> async receipt and lens verification
```

No RPC, metadata fetch, websocket backend lookup, filesystem, dashboard,
Supabase, Telegram, or JSON parsing belongs between feed receipt and a future
trade decision. Flap's `wss://flap.rcto.fun/latest` can be a control-plane
cross-check, never the authoritative hot-path dependency.

### Initial allowlist

For the first observer/paper profile, admit only:

- direct canonical Portal proxy;
- exact implementation/runtime/version pins above;
- standard `newTokenV5` with the standard token implementation;
- native ETH quote only;
- zero tax and zero extension;
- non-vault, non-staged path;
- current CDP V2 curve fields decoded from calldata/events and later verified;
- current UI-selected V2 migrator only after its DEX factory/router/pool
  derivation is independently pinned.

Observe but reject from trading: Tax V1/V2/V3, asymmetric tax, VaultPortal,
unknown extensions, ERC-20 quote tokens, native-to-quote conversion, staged
launches, V3/V4/CL migration, unknown DEX IDs or LP profiles, and any runtime or
implementation drift.

## Tier decision

### Tier 1 — no

Not enough fail-closed coverage exists for universal low-latency trading. The
upgrade surface, incomplete graduation mapping, variant count, and unverified
VaultPortal implementation are material blockers.

### Tier 2 — recommended

Build a read-only feed observer plus receipt/lens verifier and exact local curve
oracle for one simple allowlisted profile. Measure:

- feed arrival to strict decode;
- deterministic prediction agreement;
- local quote versus Portal simulation;
- launch receipt/event completeness;
- launch transaction to first eligible curve buy;
- Portal direct versus aggregator route coverage;
- actual migration events and pool identity by profile;
- proxy/version/config drift.

Promotion requires zero prediction/quote/profile mismatches across a meaningful
sample, complete migration-route proofs, stable pins, and a separately approved
paper/canary plan.

### Observe-only — fallback condition

Remain observe-only if proxy/module hashes drift, lens state disagrees with
events, an unknown extension/vault/quote/DEX appears, or migration identity
cannot be proven. Unknowns fail closed immediately.

## Primary evidence

- [Flap Robinhood production board](https://flap.sh/robinhood/board)
- [Flap deployed-address documentation](https://docs.flap.sh/flap/developers/deployed-contract-addresses)
- [Flap indexing guide](https://docs.flap.sh/flap/developers/wallet-and-terminal-and-bot-developers/index-token-created-events)
- [Flap trade guide](https://docs.flap.sh/flap/developers/wallet-and-terminal-and-bot-developers/trade-tokens)
- [Flap bonding-curve documentation](https://docs.flap.sh/flap/developers/basic-and-mechanism/bonding-curve)
- [Flap migration documentation](https://docs.flap.sh/flap/developers/wallet-and-terminal-and-bot-developers/token-migration)
- [Flap Portal vs VaultPortal](https://docs.flap.sh/flap/developers/basic-and-mechanism/portal-vs-vaultportal)
- [Flap vault specification](https://docs.flap.sh/flap/developers/vault-developers/vault-and-vaultfactory-specification)
- [Robinhood Chain connection documentation](https://docs.robinhood.com/chain/connecting/)
- [Portal proxy on Blockscout](https://robinhoodchain.blockscout.com/address/0x26605f322f7fF986f381bB9A6e3f5DAb0bEaEb09)
- [Portal implementation on Blockscout](https://robinhoodchain.blockscout.com/address/0xd9C9981D784A3765D8264D6104650B901C4e36b1)
- [VaultPortal proxy on Blockscout](https://robinhoodchain.blockscout.com/address/0xe9F7AB7DE8FB8756acbB6a1cd13316a43308197B)

## Remaining unknowns before any Tier 1 proposal

- Complete proxy admin/upgrade authority and monitoring response.
- Verified VaultPortal source and all enabled vault factory permissions.
- Exact V2/V3/V4 factory, router, init-code hash, fee tier, locker, and LP
  ownership for every active DEX/migrator profile.
- Complete graduation counts and liquidity after scanning Portal CL events and
  external migrators.
- Per-token quote/decimals normalization for economic volume.
- Whether all Robinhood frontend/user routes are present in the Nitro feed with
  sufficient lead time for a safe follower transaction.
- Exact ordering/revert behavior around curve completion and migration.
- Tax processor, extension, permit, anti-farmer, max-buy, and vault behavior by
  version under adversarial cases.
- A quiet-window end-to-end latency and paper-fill study.

No key material, transaction broadcast, deployment, service mutation, commit,
or push was performed for this research.
