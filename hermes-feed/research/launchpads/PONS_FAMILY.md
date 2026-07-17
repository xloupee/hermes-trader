# pons.family on Robinhood Chain

Research snapshot: **2026-07-16 04:51:43 UTC**. Chain ID: **4663**. This is a
read-only integration assessment; it does not authorize broadcast.

## Decision

**Recommendation: Tier 2 (observe and paper only), not Tier 1.**

Pons is unusually close to Hermes's existing NOXA shape: a strict
`launchToken` call creates a fixed-supply token with CREATE2, creates a
one-sided Uniswap v3 pool, mints the initial position, and transfers the LP NFT
to a locker in the same receipt. The launch transaction is visible in the
Robinhood Nitro transaction feed, and the canonical factory, DEX configuration,
locker, events, fee tier, restrictions, and deployed runtimes can be pinned.

It is not ready for signed execution. Pons has no official contract registry,
audit, or runtime commitment. The legacy factory is explorer-verified, but the
current factory is not. Its launch-configuration tuple differs semantically
from the NOXA tuple Hermes currently decodes, there is no unambiguous
graduation event, and public evidence did not establish a stable frontend trade
route. Tier 2 should therefore mean factory-event observation, receipt
hydration, deterministic-address research, and paper quoting only.

## What is verified

### Product and official surface

- Pons calls itself a noncustodial interface for fixed-supply token launches on
  Robinhood Chain; every transaction is wallet-submitted. Its create UI shows a
  **0.0005 ETH** launch fee, **4.2 ETH** graduation, **Uniswap / ETH**, and
  **locked liquidity** ([create UI](https://pons.family/launchpad/create),
  [llms.txt](https://pons.family/llms.txt)).
- The create form accepts token metadata, developer buy, and a creator wallet.
  The UI says that wallet receives creator fees and the developer buy. It does
  not publish the fee split or claim mechanics.
- The explore page says it shows tokens still climbing toward graduation
  ([explore](https://pons.family/launchpad)). The analytics page defines volume
  as paired-asset volume and launches as newly indexed pons markets, but its
  public snapshot showed zero / indexing and is not an activity oracle
  ([analytics](https://pons.family/analytics)).
- The public site and `sitemap.xml` publish no contract registry, ABI, source
  repository, audit, or developer documentation. The terms also warn that
  displayed quotes and fees may differ from execution
  ([terms](https://pons.family/terms)).
- The frontend pins Robinhood Chain 4663 and canonical WETH
  `0x0Bd7D308f8E1639FAb988df18A8011f41EAcAD73`. Its privacy policy names
  Robinhood RPC, Blockscout, Envio, and IPFS among its infrastructure
  ([privacy](https://pons.family/privacy)).

### Canonical contracts and provenance

| Role | Address | Evidence |
|---|---|---|
| Legacy Pons launch factory | `0xA5aAb3F0c6EeadF30Ef1D3Eb997108E976351feB` | Explorer-verified `PonsLaunchFactory`; included by the Dune token-creation query ([contract](https://robinhoodchain.blockscout.com/address/0xA5aAb3F0c6EeadF30Ef1D3Eb997108E976351feB?tab=contract)). |
| Pons launch factory and launch entrypoint | `0x0c37a24F5D23A486FA692d1500881d698B1F77a4` | Deployed by `0xda4bCee76B29EFEc9697Fcf663601c2042043968` in block 8,600,612 at 2026-07-13 10:37:10 UTC; all counted Pons markets emit its `TokenLaunched` event ([deployment](https://robinhoodchain.blockscout.com/tx/0xec8a7f6d96e30abdf5e4fb1aceaba014cd9bffce82c09337cf6ec3545c01aa45)). |
| Legacy LP locker | `0x736D76699C26D0d966744cAe304C000d471f7F35` | `locker()` on the verified legacy factory. |
| Pons LP locker | `0x31ca5E101941A93A7DD6d0497928700625CF54B5` | Receives the Uniswap v3 position NFT and emits `PositionLocked` in a canonical launch receipt. |
| Uniswap v3 factory | `0x1f7d7550B1b028f7571E69A784071F0205FD2EfA` | Factory config ID 0 and `PoolCreated` receipts. |
| Uniswap v3 position manager | `0x73991a25C818Bf1f1128dEAaB1492D45638DE0D3` | Factory config ID 0; mints the launch position. |
| Uniswap SwapRouter02 | `0xCaf681a66D020601342297493863E78C959e5cb2` | Factory config ID 0. |
| Quote asset | `0x0Bd7D308f8E1639FAb988df18A8011f41EAcAD73` (WETH) | Launch config ID 0 and launch receipts. Native ETH is the user-facing asset. |
| Robinhood wallet aggregator, observed but not Pons-owned | `0x65050A9b7E5075A2bA5cED7b1b64EE66262c40Dc` | Public pool swaps and transaction calldata; Hermes already treats it as an observed, upgradeable aggregator. |

The launch factory is both the creator/factory and the launch coordinator. No
separate Pons bonding-curve or Pons trade-router contract was established. Each
market's Uniswap v3 pool is the executable curve.

### Runtime pins at the snapshot

These are `keccak256(eth_getCode(address, "latest"))` values from the public
Robinhood RPC. They are evidence pins, not an official Pons commitment.

| Runtime | Bytes | Keccak-256 |
|---|---:|---|
| Pons factory | 24,192 | `0x921a0d1b2d854de5435804e8ee118658f05173a0eeebca5f41b41385b97cd1b5` |
| Legacy Pons factory | 24,353 | `0x0a62b8ed1d88d30c7b342ea8361dfaf0ac336706992cf0c8ba38b129f06391d4` |
| Pons locker | 4,861 | `0x5bfb52957c2df2cc05b894cd707811c811ee0e38b4a26ea59bae08cd65b39bbd` |
| Uniswap v3 factory | 24,535 | `0xec72b1abd1f2faee020cfea9c646bd8994f9fb389054f6e574f103a895091739` |
| Position manager | 24,384 | `0x0a493d1af3d0f25fed8efa205244ebee14114267a08647fc38c515c7cd6ead4f` |
| SwapRouter02 | 24,497 | `0x6f36c378e272c6324c48f045182bcb54bd8ad654cf9ebd42e8893d52c4cb25dc` |
| WETH | 2,202 | `0x5706be52f64875fee65a2cec0d80e47a23d8793cbe85d214b48445e2d05f5353` |

Token and pool runtimes contain deployment-specific immutables, so a single
raw runtime hash is not universal. The receipt-linked Ponshood proof pair is token
`0x432C99bBD9dc1d9040087598d7Cf40502d7cC20b` (5,274 bytes,
`0x6a6e5415effa82c5d552033ae9d09e8d94409b939f3b59fd829fecf363aafe74`)
and pool `0xA1ad01da59552689835902B878ce6F5eA37F2B0B` (22,142 bytes,
`0x2e70e6b2e6201475cd3eee698bd00f5db4eab8a05c4aeb0bc3897a182515b72e`).
Tier 2 should additionally derive and pin normalized creation-code hashes before
predictive admission.

### Creation, curve, and events

- Creation calldata is
  `launchToken((string,string,string,string,(string,string,string,string,string),address),uint256,uint256,bytes32)`,
  selector **`0x686399cb`**, payable. The canonical example paid the exact
  0.0005 ETH launch fee with no developer buy
  ([transaction](https://robinhoodchain.blockscout.com/tx/0xcce2b414f04ad3caab0ad38bc10cc1ac0741ed95ac740495535b71c8302fcc41)).
- The factory runtime executes CREATE2 (`0xf5`). The caller-supplied salt is part
  of the launch calldata. Verified legacy source constructs the full token
  creation code plus constructor arguments and uses the standard
  `keccak256(0xff || factory || salt || initCodeHash)[12:]` formula. Selector
  **`0xea9d3fdc`** predicts the address. Hermes must still prove that the
  unverified current runtime preserves the exact init-code construction.
- Configuration ID 0 returned: WETH; **4.2 ETH**; signed initial tick
  **-204,200**; supply **1,000,000,000 tokens at 18 decimals**; max-wallet
  **200 bps**; max-transaction **220 bps**; restriction window **366 L1
  blocks**; and three flags `false, true, false` whose meanings are unknown.
- DEX config ID 0 returned `uniswap v3`, the factory/position manager/router
  above, fee **10,000 (1%)**, tick spacing **200**, enabled `true`.
- A launch atomically emits `TokenDeployed`, Uniswap `PoolCreated`, pool
  `Initialize` and `Mint`, locker `PositionLocked`, and final `TokenLaunched`.
  Relevant topics are:
  - `TokenDeployed`: `0x1461370115e1c2be79cb529f8cfcbd11316e789d9c6099fc83417b0b4c48c62a`
  - `PoolCreated`: `0x783cca1c0412dd0d695e784568c96da2e9c22ff989357a2e8b1d9b2b4e6b7118`
  - `PositionLocked`: `0xf3fabcec8f79e4c84abcb646b5b7eb0af5fa1fcc77977e928d8b87562cc96904`
  - `TokenLaunched`: `0xdb51ea9ad51ab453a65a4cb7e60c3cb378c9501bb002609f8f97778fb6c4235a`
  - Uniswap v3 `Swap`: `0xc42079f94a6350d7e6235f29174924f928cc2ac818eb64fed8004e115fbcca67`
- The initial LP is one-sided token liquidity and its NFT is transferred to the
  locker at launch. Trading occurs directly on that permanent Uniswap v3 range;
  verified legacy source has no separate pre-DEX curve or later DEX migration.

### Trading semantics

- The canonical direct-router selectors are `exactInputSingle` **`0x04e45aaf`**
  and `exactOutputSingle` **`0x5023b4df`**. The Robinhood aggregator selector
  **`0x4d819a2a`** is also visible in real Pons-pool transactions. Other public
  routers/bots call the pools, so a pool allowlist is stronger attribution than
  one `to` address.
- Native-ETH aggregator buys carry `msg.value`; the route wraps to WETH. A
  pre-wrapped direct-router buy uses WETH approval and zero value. Sells use
  zero value and require ERC-20 approval to the selected router/aggregator.
- An atomic developer buy uses `msg.value = launchFee + initialBuyAmount` and
  the factory calls `SwapRouter02.exactInputSingle` WETH-to-token with
  `amountOutMinimum = 0`. The factory approves the position manager for supply,
  mints, and clears that approval. Ordinary frontend sell multicall/unwrap
  details remain unverified.
- Complete current-generation receipts now cover both launch orientations and
  atomic buying. Transaction `0xb77e066e...00bc47` proves a token-above-WETH
  launch with a 0.001 WETH initial buy; transaction `0xeb622138...dc519`
  proves token-below-WETH initialization and a nonzero initial buy. Their
  initialize, one-sided mint, locked LP NFT, and terminal V3 swap states are
  reproduced exactly by the paper quote verifier.
- Verified costs/restrictions are the 0.0005 ETH launch fee, 1% Uniswap pool
  fee, 2% max wallet, 2.2% max transaction, and 366-L1-block restriction
  window. The contract-level meanings of the three config flags, creator-fee
  split, locker fee claims, and any post-restriction policy are unknown.
- No fee-on-transfer tax was established. Treat “no token tax” as an inference,
  not a verified protocol promise.
- Verified legacy token logic applies restrictions only to canonical-pool buys:
  non-exempt launch-block buys revert, and the 366-block window enforces the
  wallet and cumulative-bought caps. The atomic initial-buy recipient has a
  narrow exemption; ordinary transfers and sells are not capped. Current
  runtime compatibility is strongly evidenced but not source-verified.

## Graduation and migration

**Verified:** the factory stores 4.2 ETH in launch config ID 0; the initial
fixed supply is placed in a one-sided, locked Uniswap v3 position; the pool is
tradeable from launch. Verified legacy source computes “graduation” as a
milestone from paired-asset principal remaining in that locked position. There
is no proprietary pre-DEX curve and no later DEX migration.

**Unknown:** whether the unverified current factory changed any edge semantics,
the exact indexer/UI state transition, whether fees influence the displayed
progress, and whether any post-graduation liquidity reshaping exists outside
the launch factory. No canonical graduation event or migration selector was
verified. Consequently the 24h/7d graduation count is unknown.

## Activity snapshot and the Dune 15,494 number

The reproducible current-generation market universe is the set of
factory-specific `TokenLaunched` logs, not tokens merely displayed by pons and
not every chain call sharing selector `0x686399cb`.

Using Blockscout's logs API with:

```text
address = 0x0c37a24F5D23A486FA692d1500881d698B1F77a4
topic0  = 0xdb51ea9ad51ab453a65a4cb7e60c3cb378c9501bb002609f8f97778fb6c4235a
fromBlock = 0
toBlock = latest
snapshot = 2026-07-16T04:51:43Z
24h cutoff = 2026-07-15T04:51:43Z
```

the result was:

| Metric | 24h | 7d / lifetime-to-snapshot |
|---|---:|---:|
| Canonical Pons markets launched | **319** | **327** |
| Unique creators (`TokenLaunched.deployer`) | **63** | **68** |

Of the 327 launches, **192** contained a nonzero atomic initial buy and **135**
did not. Launch-time initial buys totalled approximately
**41.76458550724637 ETH**.

The first canonical event was block **8,621,658** at
**2026-07-13 11:12:08 UTC**; the last included event was block **10,926,232**
at **2026-07-16 03:16:03 UTC**. Because the factory was deployed inside the
7-day window, 7d and lifetime are equal at this snapshot. The raw evidence is
in `PONS_FAMILY_EVIDENCE.json` and is reproducible through
[Blockscout's API](https://robinhoodchain.blockscout.com/api?module=logs&action=getLogs&fromBlock=0&toBlock=latest&address=0x0c37a24F5D23A486FA692d1500881d698B1F77a4&topic0=0xdb51ea9ad51ab453a65a4cb7e60c3cb378c9501bb002609f8f97778fb6c4235a&page=1&offset=1000).

The public Dune dashboard is [adam_tehc/pons](https://dune.com/adam_tehc/pons).
Its **15,494** “total tokens created” value comes from
[Dune query 7982654](https://dune.com/queries/7982654),
visualization **11967333**:

```sql
SELECT COUNT(DISTINCT address) AS total_tokens
FROM robinhood.creation_traces
WHERE "from" IN (
  0xa5aab3f0c6eeadf30ef1d3eb997108e976351feb,
  0x0c37a24f5d23a486fa692d1500881d698b1f77a4
)
AND block_time >= TIMESTAMP '2026-07-01'
```

Thus 15,494 is an all-generation, since-July-1 distinct child-contract count
for the **legacy plus current** Pons factories. It is not a 24h/7d count and not
the current factory's market count. Verified factory source shows the Pons
factory CREATE2-deploys the token while the Uniswap factory creates the pool,
so the child contracts are strongly attributable as Pons-created token
contracts—not arbitrary tokens merely indexed by the frontend. Current-factory
events account for 327; subtracting implies **15,167 legacy tokens**, but that
legacy split remains an inference because the independent legacy event scan hit
Blockscout's 1,000-row cap. Dune's number is therefore plausibly an actual
all-generation token total, while 327 is the event-confirmed current-generation
total.

Bounded public-RPC aggregation did not finish a complete union of the legacy
and current pool logs/states. Therefore **all-generation 24h/7d launches and
unique creators, plus unique traders, swaps, paired-asset volume, current
liquidity, and graduations are unknown**, rather than estimated from partial
pools. The table above is explicitly current-generation only. A follow-up
indexer query should join canonical Pons tokens to
their `TokenLaunched.pool` values, then aggregate `Swap` senders/transactions,
absolute WETH legs, WETH pool balances, and a separately proved graduation
predicate.

## Feed visibility and Hermes seams

The launch is visible before receipt in the Robinhood transaction feed as:

```text
to = Pons factory
selector = 0x686399cb
value >= pinned launch fee
```

The public mainnet feed is `wss://feed.mainnet.chain.robinhood.com`; confirmed
state and receipts are available from
`https://rpc.mainnet.chain.robinhood.com`. Neither belongs on the decision path
as an on-demand lookup: the websocket is already connected and RPC proof stays
asynchronous.

Hermes can reuse these seams without adding hot-path I/O:

1. `decoder.rs`: add both Pons factory generations and the launch selector to
   the precomputed address and selector filters. Recover the sender only after
   the cheap match, and label the generation.
2. A new Pons ABI/config module: strict full `launchToken` decode; never reuse
   the NOXA launch-config tuple decoder because word 1 is the 4.2 ETH field,
   not NOXA's `dexId`.
3. A Pons predictor: startup-pin factory, locker, DEX config, launch config,
   normalized creation code, owner/enabled/fee, and the Uniswap dependencies;
   prove the current factory's exact CREATE2 init-code construction before
   prediction.
4. Receipt verifier: require exactly one factory `TokenLaunched`, matching
   `PoolCreated`, ordered initialize/mint, LP NFT transfer to the pinned locker,
   `PositionLocked`, and no unexpected burn/migration.
5. Paper registry: learn verified pool/token pairs asynchronously and reuse the
   existing Uniswap v3 state/quote machinery. Aggregator or watched-wallet
   observation should normalize to the pinned pool, WETH pair, fee, and signer.
6. Keep all RPC, Blockscout, Envio, analytics, Telegram, database, filesystem,
   and graduation/indexer work off the candidate path. The feed hot path should
   only decode, match, classify, predict/quote from warm state, and paper-arm.

### Promotion gates

- **Tier 2 now:** factory launch observer, receipt proof, pool registry, and
  paper quotes/trades with runtime/config drift halting the observer.
- **Tier 1 later:** independently prove normalized token creation code and
  CREATE2 formula; expand both-orientation and initial-buy sampling beyond the
  retained live proofs;
  define graduation from onchain state; audit restriction flags and locker fee
  claims; verify buy/sell calldata/value/approval variants; obtain a quiet-window
  sample with zero prediction/quote mismatches; then require separate explicit
  canary authorization.

## Unknowns retained deliberately

- Official ownership/endorsement of the deployed addresses and upgrade policy.
- Current-factory verified source/ABI, audit, and normalized creation-code hash.
- Meaning of the three launch flags and exact restriction enforcement.
- Creator/platform fee split and locker claim/withdraw behavior.
- Stable frontend router choice and exact slippage/deadline policy.
- Contract-level graduation predicate, graduation event, and any migration or
  post-graduation liquidity operation.
- Complete 24h/7d traders, swaps, volume, liquidity, and graduation aggregates.
- Independent complete legacy event count and the inferred 15,167 legacy split.
