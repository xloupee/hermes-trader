# Robinhood Chain launchpad ecosystem ranking for Hermes

Evidence snapshot: **2026-07-16 04:00–05:00 UTC**. Target network: **Robinhood Chain mainnet, chain ID 4663**. Base mainnet is **8453** and Solana is not EVM; activity on either is never used below as proof of Robinhood support.

This is a research ranking, not an enablement decision. No key was accessed, no transaction was broadcast, and no service or deployment was changed.

## Decision

| Priority | Candidate | Hermes tier | Actual role on chain 4663 | Opportunity and adapter rationale |
|---:|---|---|---|---|
| 1 | **bow.fun** | **Tier 1** | Native launch factory; single-sided canonical Uniswap V3 from launch | Best combination of real follow-on activity and reuse: 17,480 swaps/24h, 94 launches/24h, 294 active tokens/24h, and the existing Hermes V3 pool path is reusable. |
| 2 | **LaunchHood V3** | **Tier 1** | Native factory; canonical Uniswap V3 pool and locked LP created at launch | 5,914 indexed trades and 143.82 ETH notional/24h. Factory discovery is new, but quoting/swapping can reuse Hermes V3. Suppress embedded initial buys and launch spam. |
| 3 | **Clanker v4** | **Tier 1** | Native Clanker factory plus configurable Uniswap V4 hooks/extensions | Large, established 4663 firehose (officially 1.6k launches/24h, 4.7k/7d), but execution needs a new V4 PoolManager/hook adapter and per-token extension risk parsing. |
| 4 | **Bankr / Doppler** | **Tier 1** | Bankr is the deploy/API/fee layer; Doppler + Uniswap V4 are the underlying protocol/venue; launches may be ERC-4337 wrapped | Strategic second consumer of a generic V4 adapter and high distribution velocity: 44 Robinhood launches in the latest 50 Bankr records over 79 minutes. Full period swap/volume data is unknown, so enrich asynchronously and score on observed pool activity. |
| 5 | **trench.today** | **Tier 1 discovery; execution gated** | Native upgradeable curve/router path with later Uniswap venue | Official API reports ~$371,953 24h volume, but proxy implementation ABI, exact buy/sell selectors, curve math, taxes/restrictions, upgrade authority, and graduation derivation must be recovered before paper execution. |
| 6 | **klik.finance** | **Tier 1 discovery; Tier 2 execution** | Native CREATE2 factory; immediate locked Uniswap V4 liquidity through a Klik dynamic-fee hook | At least 512 launches/24h and at least 4,395/7d. Discovery is trivial; execution waits for generic V4 plus hook-aware dynamic-fee quoting. Raw issuance is not proof of tradable density. |
| 7 | **hood.fun** | **Tier 2** | Native constant-product ETH bonding curve, then canonical Uniswap V3 migration | 96.26 ETH indexed 24h volume, but only 93 of 7,313 new tokens had nonzero 24h volume. Build after Tier 1 and require follow-on activity/liquidity filters. |
| 8 | **leavehood.com** | **Tier 2** | Native upgradeable bonding curve, then Uniswap V3 graduation | 208 confirmed launches but only 34 direct curve swaps/24h. Useful V3 downstream reuse, but low current tradable density and incomplete curve/migration ABI. |
| 9 | **Virtuals** | **Tier 2 attribution; observe-only protocol** | Current `... by Virtuals` tokens on 4663 are Bankr/Doppler launches, not a Virtuals factory | Add an attribution signal to the Bankr adapter. Do not port Base-8453 BondingV5 selectors to Robinhood without a separately verified 4663 deployment. |
| 10 | **LaunchHood legacy curve** | **Observe-only** | Separate ETH curve and event bus, then Uniswap V4 graduation | Only 17 launches, seven with more than one indexed trade, and two graduations/7d. Separate protocol from LaunchHood V3; low incremental opportunity. |
| 11 | **bags.fm** | **Observe-only** | Proven Solana launchpad; no Bags-owned 4663 protocol proven | Robinhood terminal/front-end claims do not identify a 4663 factory, router, event, or creation transaction. |
| 12 | **ape.store** | **Observe-only** | Proven Base-8453 launchpad; no 4663 protocol proven | Base curve/graduation mechanics are not Robinhood evidence. |
| 13 | **long.xyz** | **Observe-only** | Proven Base-oriented auction platform; no 4663 deployment proven | No reproducible Robinhood contract, transaction, code, or log evidence. |

The LaunchHood **NOXA tab is only a frontend/indexer surface** for NOXA coins. It is not a LaunchHood deployer or protocol and must not create a duplicate adapter or duplicate launch count.

## Why this order

Hermes already has a chain-4663 sequencer feed, active N0xa factory discovery, CREATE2 prediction, canonical Uniswap V3 pool reconstruction/quoting, and V3 router intent decoding. It has **no generic Uniswap V4 PoolManager/hook adapter** in this commit. Therefore:

1. Bow and LaunchHood V3 offer the fastest path from factory discovery to executable V3 opportunity.
2. Clanker, Bankr/Doppler, and Klik justify one shared V4 foundation, but each still needs protocol-specific hook/config validation.
3. Curve launchpads need separate pre-graduation state/quote logic even if their post-graduation pools reuse V3.
4. Launch volume is discounted unless supported by follow-on swaps, distinct traders, volume, or executable liquidity. Several platforms are heavily spam/Sybil inflated.

## Chain identity and attribution gate

Every adapter must retain these roles separately:

- **frontend/source**: website, bot, API, SDK, or campaign label;
- **transaction sender**: EOA, smart account, or ERC-4337 account;
- **bundler/EntryPoint**: transport wrapper, never assumed to be creator or factory;
- **protocol factory/router**: the contract whose code and events define the launch;
- **underlying venue**: curve contract, Uniswap V3 pool, or Uniswap V4 PoolManager/pool ID.

Reproducible network check:

```sh
cast chain-id --rpc-url https://rpc.mainnet.chain.robinhood.com
# 4663
```

Official network reference: <https://docs.robinhood.com/chain/connecting/>.

## Tier 1 dossiers

### bow.fun — V3-first, strongest reuse

Official sources: <https://bow.fun/docs.html>, <https://bow.fun/>, <https://bow.fun/api/metrics>.

- Factory `0xC70E510E14710Ea535CAB7b2414860aF63FEab79`; locker `0x904dCCB96d877E6db365282251Fa3dD156476660`; optional `BowZap` `0xCCA95E5442BbF175d8a1Ad136Be317fA6D55CC38`.
- Canonical WETH `0x0Bd7D308f8E1639FAb988df18A8011f41EAcAD73`, V3 factory `0x1f7d7550B1b028f7571E69A784071F0205FD2EfA`, router `0xCaf681a66D020601342297493863E78C959E5cb2`, position manager `0x73991a25C818Bf1f1128dEAaB1492D45638DE0D3`.
- Launch ABI: `launch((string,string,uint256,uint256,uint256,uint256,uint256,bytes32,string,string,string,string,string,string,uint256)) payable`; discovery event `Launched(address indexed token,address indexed deployer,address pool,uint256 positionId,uint256 launchId)`.
- CREATE2 confirmed; the UI mines salts for the `b03` suffix, which is a secondary check only.
- No ERC-20 mint or transfer tax. Per-launch tuple carries launch delay, max-wallet, and limit window; docs describe a 2% initial cap and creator dev-buy exemption.
- Liquidity exists in a single-sided V3 position from launch. The 3.7 ETH `GRADUATION_WETH()` latch marks progress; it is not migration to a different DEX.
- Activity: 1,054 total tokens, 530 creators, 19 lifetime graduates; 17,480 swaps/24h; 94 launches and 55 distinct deployers in the measured 24h; at least 1,050 timestamped launches/7d. Dollar liquidity and period graduations remain unknown.

Feed plan: decode `Launched`, register the pool immediately, reuse Hermes V3 state/quote/swap, read token-specific limits, and treat BowZap as optional until its bytecode is pinned.

### LaunchHood V3 — V3-first, filter launch automation

Official sources: <https://launchhood.com/docs>, <https://launchhood.com/>, public GraphQL at `https://launchhood-indexer-production.up.railway.app/graphql`.

- Factory `0x62B33A039D289CBDa50EbeB72Fe4261449E61Bcf`; locker `0x99B79154Ff4Fc0e313549B809254B02722631ee0`; token implementation `0x5FDf73abC7A232d91b03638c2f9a52c16aB0E3bE`.
- `launchToken((name,symbol,metadataURI,rewardRecipient),configId,dexId,userSalt,minTokensOut)` selector `0x4110a41c`; emits `TokenDeployed` and `TokenLaunched(token,deployer,pool,...)` (topic begins `0x235e34a4`).
- CREATE2 confirmed. Launch creates the canonical WETH/V3 pool and locked position in one transaction; there is no curve or later migration.
- Config 0: 1B supply, 1% V3 pool fee, tick spacing 200, temporary 2% max-wallet for 366 blocks, no max-tx limit, no token tax, zero launch fee. Optional initial buy occurs before restrictions activate.
- Activity: 2,278 launches/24h from 60 creators, but only 307 launch rows had `tradeCount > 1`; 5,914 indexed trades, 953 traders, 2,285 traded tokens, and 143.816 ETH notional/24h. Seven-day launches: 2,819 V3 plus 17 legacy curve. Exact 7d trades/volume and current liquidity are unknown; a partial aggregation is not promoted to an exact metric.

Feed plan: factory discovery plus existing V3 adapter. Mark the embedded launch initial buy so it does not count as organic momentum; enforce the 366-block limit in simulation.

### Clanker — native V4 protocol

Official sources: <https://www.clanker.world/clankers/chain/robinhood/stats>, <https://github.com/clanker-devco/clanker-sdk>, <https://github.com/clanker-devco/v4-contracts>.

- V4 factory `0xD3f2cC1731b7Fd17f28798835C2E02f0a1839A94`; runtime code hash observed `0xf895112a2deed34ba2765d0147aff3494104a28293cc2f19af9275934088da33`.
- PoolManager `0x8366a39cc670b4001a1121b8f6a443a643e40951`; WETH as default quote; locker `0x290F735F63824BB5836cDe24a35F5103A5B5Bc99`.
- Deploy selector `0xdf40224a`; `TokenCreated(...)` topic `0x9299d1d1a88d8e1abdc591ae7a167a6bc63a8f17d695804e9091ee33aa89fb67`.
- Live proof: tx `0x1237de969043eee811d7bafdcdcbbb149216016b7417477f7a43d2561dc5167e`, token `0x6bBBb3Be7424a911D5D131E272639512C1c12b07`, pool ID `0x99cdbc6f39e5b75958247787f30c59f251301f3fa517c36456e72b350c546d03`. The `charms` interface and direct sender are distinct from the factory.
- Immediate configurable V4 concentrated liquidity, not a curve/graduation flow. Static/dynamic hooks, MEV descending fees, vault, airdrop, dev-buy, fee recipients, paired token, and position ranges are token-specific risk fields. CREATE2 is strongly suggested by salt/vanity tooling but not made an execution invariant here.
- Official activity: 1.6k launches/24h, 4.7k/7d, site-labeled 1.0k transactions/24h, 0.071 WETH protocol revenue/24h, 2.10 WETH/7d. Exact unique creators, clean swaps, aggregate volume, and liquidity are unknown.

Feed plan: decode factory event, validate quote/hook/locker/extensions, register pool ID in a generic V4 adapter, then consume PoolManager swap events by pool ID.

### Bankr / Doppler — ERC-4337-aware V4 distribution

Official sources: <https://docs.bankr.bot/token-launching/overview/>, <https://docs.bankr.bot/token-launching/api-reference/deploy-token-launch/>, <https://api.bankr.bot/token-launches>, <https://docs.doppler.lol/>.

- Bankr supports Robinhood wallet-auth launches; partner-key launches remain Base-only. The 4663 launch market uses Doppler and Uniswap V4, not a separate Bankr AMM.
- Proof tx `0xc6597fe88f8f3f16b4ba6613c25050d75dc6f3c2b2c5315f0b47828f98f0609c`: outer `to` is ERC-4337 EntryPoint `0x0000000071727de22e5e9d8baf0edac6f37da032`, selector `0x765e827f`; bundler `from` is `0x256b3cc1e516d124b3027ecd083aa5a940d1328e`; user-operation sender/API deployer is `0xff89978cb8171132395741b785d4a1f7e3efa124`.
- The sender is an EIP-7702 delegated account. Its exact designator is `0xef0100d6cedde84be40893d153be9d467cd6ad37875b28` (hash `0x4542cbf1da24ba964614e4f5585736e22884e23e97c0f0915de4f602585d2dd4`). The delegated Kernel implementation is `0xd6cedde84be40893d153be9d467cd6ad37875b28`, with runtime hash `0x6f6d6691dc11fda98d3102802f20e7e816ccc576c16c9279ee1a884a51d1935d`. Both identities must be pinned; the designator is not the delegated runtime.
- The proof account profile is ERC-7579 `execute(bytes32,bytes)` selector `0xe9ae5c53`, all-zero single-call/revert mode, target `0xeb7c034704ef8dcd2d32324c1545f62fb4ad0862`, native value zero, and inner selector `0x882db707`. Selector acceptance is per-account-profile, not global.
- Doppler `Create` emitter `0xeb7c034704ef8dcd2d32324c1545f62fb4ad0862`; PoolManager `0x8366a39cc670b4001a1121b8f6a443a643e40951`; WETH quote; resulting token `0x3441a266d52d42805714cb0f3f8f309369c01ba3`; pool ID `0x733a2357a3100dca908e4cc9a76573e8bd486a8ef5a2bc881749334adaab7a5c`.
- Useful topics: Doppler `Create(address,address,address,address)` `0x68ff1cfcdcf76864161555fc0de1878d8f83ec6949bf351df74d8a4a1a2679ab`, `Lock(address,(address,uint96)[])` `0x5be4f748347693e0500df872d81f7d96bce1b98e6f5adff0cfddfe3e9e415f20`, `FeeScheduleSet(bytes32,uint32,uint24,uint24,uint32)` `0xcea1bdc74004c2beebf7a8d2d531c3950ca35e8326a55bdc553df9d1b593d7b3`, and ERC-4337 `UserOperationEvent` `0x49628fd1471006c1482da88028e9ce4dbb080b815c9b0344d39e5a8e6ec1419f`.
- Immediate WETH/V4 pool; documented 0.7% swap fee split 95% creator/5% Doppler. Standard launch reserves 15% for two-year vesting with 30-day cliff; 85% seeds liquidity. Partner launches use 100% pool supply. No Bankr-specific graduation is documented. CREATE2 derivation remains unknown.
- Latest API window: 44/50 launches were Robinhood across 79m37s; 42 API deployers across all 50. Exact 24h/7d launches, creators, swaps, volume, liquidity, and graduations are unknown; do not extrapolate.

Feed plan: unwrap ERC-4337 inner calls/events, attribute Doppler factory/token/pool separately, then asynchronously enrich from Bankr. Never classify EntryPoint or bundler as factory/creator.

### trench.today — strong volume, incomplete execution surface

Official sources: <https://trench.today/>, <https://trench-today.gitbook.io/trench-today>.

- Router/protocol proxy `0x77dC6f6361b7b99456FC3761ce5b7ddA80d83f9d`, point-in-time implementation `0x6D0Ff368DB6cf9C94a182aD2375E640EC71ACEe9`.
- Token deployer proxy `0x2ECFb98BCe4f3616115E4a2A7a2379AF388DFbAA`, implementation `0xDc4b9FAF72a071E2b5A7858bF91A894580D84a22`.
- Direct proof: tx `0xd768f27995f2e3476985ffc9a3c5b7bc1df51ee27519ae3dc45d3ad19fb2d0df` at `2026-07-16T04:44:25Z`, creator/sender `0xac93f07bac60ba09a561fb8d4b4289950ddfcc70`, proxy selector `0xf39dc3ed`, 0.03 ETH value, token `0x149552683133cc173b4117645e04b3a08248cccc`.
- `0xf39dc3ed` is inferred to launch with optional initial buy. Observed `0x2ce7a0fa` and `0xae87c397` are likely trade selectors, but their signatures/directions are **not confirmed** and must not be hard-coded.
- Frontend exposes native ETH and optional USDG quote `0x5fc5360D0400a0Fd4f2af552ADD042D716F1d168`, `bonding`/`graduated` states, and Uniswap as graduated venue.
- Official cumulative stats: 8,696 tokens, 36 graduated; 24h volume $371,952.99. The capped recent list does not prove period launches/creators. Swaps, liquidity, curve formula, graduation threshold, taxes/restrictions, CREATE2, and period graduations are unknown.

Gate: discovery may record validated creation traces. Paper execution waits for verified implementation ABI/events, code/upgrade pin, curve quoting, sell behavior, and deterministic graduation pool derivation.

### klik.finance — verified V4 discovery, hook-aware execution

Official source: <https://klik.finance/docs>. Verified factory: <https://robinhoodchain.blockscout.com/address/0x16cF6788B762EE8969744586eD16fc5705140dd7>.

- Factory `0x16cF6788B762EE8969744586eD16fc5705140dd7`; `deployCoin(string,string,string,bytes32,uint256)` selector `0x4101659e`.
- Discovery event `ERC20TokenCreated(address)` topic `0x60122e78030aba0a2e4a67adb3e52b411343cc51778f919095d3fe394090c1b2`; optional `TokenPurchased` captures deploy-time buy.
- CREATE2 confirmed; `0x69` vanity prefix is optional and never sufficient attribution.
- Immediate locked single-sided WETH/Uniswap V4 liquidity with Klik hook. No curve or graduation. Dynamic fee falls from 1.00% below 15 ETH market cap to 0.10% at/above 6,000 ETH and is split between creator/platform. The docs contradict themselves on whether creator-buy penalty remains; current behavior must be read/simulated.
- Activity: 512 factory launch logs/24h. Seven-day scan established at least 4,395 but hit a capped subrange, so the 7d number is a lower bound. Creators, swaps, volume, liquidity are unknown; graduation is not applicable.

Feed plan: add factory event discovery now; enable execution only after generic V4 state/quote plus current Klik hook/config validation.

## Tier 2 dossiers

### hood.fun — real curve, sparse tradable output

Official sources: <https://www.hood.fun/whitepaper>, <https://www.hood.fun/>. Verified current factory: <https://robinhoodchain.blockscout.com/address/0x5fcc1df0dc020cf454e742e9a8ae2554c37a452c>.

- Current `HoodCustomLaunchpad` `0x5Fcc1DF0dC020CF454e742E9a8Ae2554C37A452C`; locker `0xad69d8A00564f4A2365cc74594925f95281706Aa`.
- `createToken(...)` selector `0x42b62137`; `buy(address,uint256)` `0xcce7ec13`; `sell(address,uint256,uint256)` `0x6a272462`; factory emits `TokenCreated`, `Trade`, `Graduated`, and `Migrated`.
- CREATE2 confirmed; current vanity enforcement produces `600d` suffix, used only as secondary validation.
- Native ETH constant-product virtual curve: 1B supply, 800M on curve, 200M for V3; virtual reserves 2.81 ETH/1.145B tokens; about 6.5 ETH raise. Creator chooses 1–5% curve fee; migration fee is 0.05 ETH + 3%; post-migration V3 pool is 1%.
- Transfers locked before graduation. Live `guardBlocks=100` but `guardMaxWalletBps=0`, so the advertised snipe cap was configured off at snapshot. No token transfer tax.
- Board snapshot: 7,313 launches, 2,857 creator addresses, two migrations, 96.263 ETH indexed volume/24h, but only 93 tokens with nonzero 24h volume. Seven-day: 8,367 launches, 3,538 creators, 17 migrations; exact 7d swaps/volume unknown.

### leavehood.com — low-density curve/V3

Official API/contracts: <https://leavehood.com/docs>.

- Factory proxy `0x2C81Cd8acF4886F4abAd332216b4444aE927FDb7`, implementation observed `0x7d6CcAAdc2249a21a2B404EDa2d9465E739c833B`.
- Core proxy `0x5090C9cd2228b0C4e6a83Ee44ab77Ce2e4cd89E3`, implementation observed `0x79446BCa2a86B23Cb6354178235222f491D18f56`.
- Confirmed launch selectors `0x0e1d3073` and `0xfcd0508f`; core `buyWithSlippage` `0x6784ad1e`, `sellWithSlippage` `0x0dda52f6`, `sell` `0x6c197ff5`, `claimCreatorFees` `0xd6ae6e44`.
- Native curve quotes WETH/ETH and graduates to canonical V3. Exact curve, threshold, migration event, CREATE2, restrictions, and taxes are unknown because implementations are unverified.
- Direct-call snapshot: 208 confirmed launches and 34 curve swaps/24h; 978 confirmed launches and 464 curve swaps/7d. These exclude internal calls and post-graduation V3 swaps. True creators, volume, liquidity, and period graduations are unknown.

### Virtuals — attribution only on Robinhood

Official Base code: <https://github.com/Virtual-Protocol/bondv5-trader>. It explicitly targets **Base 8453**. Its BondingV5 curve/router/selectors are not Robinhood evidence.

The sampled 4663 token `rooty by Virtuals` is the Bankr/Doppler ERC-4337 transaction documented above. The latest 50 Bankr rows contained 12 `by Virtuals` Robinhood launches from 11 API deployers, but this proves a distribution label, not a Virtuals 4663 factory. Execution inherits Bankr/Doppler mechanics. All standalone Virtuals Robinhood swap/volume/liquidity/graduation metrics are unknown.

## Observe-only notes

- **LaunchHood legacy curve**: factory `0x2e9fBf18F6492F6651B983c34629d292516DE86e`, shared event bus `0x9e56...4dE4`, native ETH curve, 3 ETH threshold, then Uniswap V4. Low recent activity and incomplete bus ABI do not justify a separate adapter now.
- **bags.fm**: official public APIs and ReStream events are Solana-native. No Bags-owned 4663 factory/router/log was proven. Terminal support can be a frontend route only.
- **ape.store**: real Base-8453 curve launchpad; its ~$69k graduation and Uniswap LP mechanics are not evidence for 4663.
- **long.xyz**: real Base-oriented auction/launch platform with later Uniswap migration; no 4663 contract/activity proof.

## Implementation sequence

1. Add **Bow** and **LaunchHood V3** factory discovery to the current V3 registry; paper-score only follow-on swaps.
2. Build a generic **Uniswap V4 PoolManager/pool-ID state and quote adapter**, then add Clanker validation first, Bankr/Doppler ERC-4337 attribution second, and Klik hook semantics third.
3. Recover and pin **Trench** proxy ABIs/upgrade authority/curve math before any execution.
4. Add hood.fun and LeaveHood curve adapters only after Tier 1 paper scoring proves useful capacity.
5. Keep Virtuals as Bankr metadata attribution; keep LaunchHood legacy, Bags, Ape, and Long on the evidence watchlist.

All hot-path configuration must be preloaded. Factory/pool/hook/code hashes, selectors, restrictions, quote state, and route config belong in warm Rust memory. Frontend/API enrichment, metrics, Telegram, database writes, and dashboards remain post-submit/control-plane work.

## Reproducibility and limitations

Representative read-only commands:

```sh
cast tx <hash> --rpc-url https://rpc.mainnet.chain.robinhood.com
cast receipt <hash> --rpc-url https://rpc.mainnet.chain.robinhood.com --json
cast codehash <address> --rpc-url https://rpc.mainnet.chain.robinhood.com
curl -sS 'https://robinhoodchain.blockscout.com/api?module=logs&action=getLogs&fromBlock=<from>&toBlock=<to>&address=<factory>&topic0=<topic>'
```

- API/indexer metrics are labeled as such; they are not silently promoted to full-chain truth.
- `from` is not automatically a human creator. Smart accounts, relayers, bundlers, and protocol contracts must be separated by receipt/log evidence.
- Proxy implementation addresses and hook/module configuration are point-in-time observations. Re-read and code-hash-pin them before implementation or canary work.
- Exact metrics remain unknown where a public endpoint was capped, pagination was unsafe, or full log replay was not completed. Lower bounds are labeled.
- “Locked,” “unruggable,” or “audited-pattern” descriptions are project claims unless independently proven; this report verifies observable mechanics, not security guarantees.
