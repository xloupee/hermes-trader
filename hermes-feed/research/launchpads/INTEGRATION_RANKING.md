# Robinhood Launchpad Integration Ranking

Snapshot: 2026-07-16 UTC. Network under review: **Robinhood Chain mainnet, chain ID 4663**. Base is **chain ID 8453** and Solana is a separate network; activity on either is excluded unless the same candidate also has contract, transaction, log, or runtime-code proof on chain 4663.

## Decision

| Recommendation | Candidates | Meaning |
|---|---|---|
| Tier 1 | **bow.fun**, **LaunchHood V3**, **Clanker**, **Bankr/Doppler** | Suitable for adapter implementation and paper validation, subject to the fail-closed pins and route constraints in the architecture document. |
| Tier 1 discovery; execution gated | **trench.today** | High measured activity warrants immediate decoding and observation work, but curve direction, quote math, sell behavior, and graduation are not yet safe to execute. |
| Tier 1 discovery; Tier 2 execution | **klik.finance** | Strong chain-4663 launch visibility; execution waits for the shared V4 adapter and validated hook/dynamic-fee semantics. |
| Tier 2 | **Pons**, **Flap**, **hood.fun**, **leavehood.com** | Observe and paper-test only until the candidate-specific promotion gates are satisfied. |
| Attribution only | **Virtuals via Bankr/Doppler** | A creator/referrer label over the Bankr/Doppler execution path, not another venue or adapter. |
| Observe only | **LaunchHood legacy curve**, **bags.fm**, **ape.store**, **long.xyz** | Insufficient current chain-4663 opportunity or proof for execution integration. |

Recommended build order is **(1) the shared static registry and V3 launch-at-birth adapter for bow.fun and LaunchHood V3; (2) the shared V4/Permit2 foundation; (3) Clanker; (4) Bankr/Doppler with bounded ERC-4337 unwrapping; (5) discovery-only decoders for trench.today and klik.finance**. This sequence optimizes safe adapter reuse; it does not claim that the first implementation has the largest attribution count.

## Evidence rules

The ranking prioritizes, in order:

1. actual follow-on swaps, normalized volume, and usable liquidity;
2. unique creators and market breadth;
3. contract and code stability;
4. visibility in the existing feed before receipts;
5. ability to identify the copied leader through direct calls, aggregators, multicalls, or smart accounts;
6. receipt-free token or pool prediction;
7. reuse of a small number of static adapters.

Unknown metrics receive no credit. Launch count is discovery evidence, not proof that a token is tradable, liquid, or copyable. Official labels and application APIs are useful discovery sources but are weaker than chain-4663 logs, transactions, and runtime code.

## July 15 Dune attribution chart

Query 7916783, `daily new tokens`, reports **47,687** creations:

| Attribution label | Count |
|---|---:|
| pons.family | 15,494 |
| flap.sh | 12,497 |
| other | 6,675 |
| hood.fun | 3,106 |
| launchhood | 2,273 |
| bankr.bot | 1,964 |
| clanker.world | 1,746 |
| virtuals | 1,044 |
| klik.finance | 682 |
| trench.today | 639 |
| bags.fm | 501 |
| ape.store | 483 |
| leavehood.com | 213 |
| bow.fun | 104 |
| long.xyz | 48 |
| Other | 218 |

These are attribution volumes only. They do not establish unique contracts, follow-on swaps, normalized volume, liquidity, creator diversity, or a copyable execution route. Labels may also describe front ends or referrers sharing an underlying protocol. The Pons report, for example, finds that its 15,494 label is a since-July-1 creation-trace count across legacy and current factories, while the current factory produced only 327 events in its measured seven-day lifetime. Virtuals is similarly an attribution overlay on Bankr/Doppler on Robinhood, so it must not become a duplicate adapter.

## Candidate ranking and rationale

| Order | Candidate | Chain-4663 activity evidence | Technical fit | Main limitation |
|---:|---|---|---|---|
| 1 | bow.fun | 94 launches, 17,480 swaps, 294 active tokens, and 55 creators in 24h; at least 1,050 launches in 7d; 19 lifetime graduates | Deterministic factory flow and V3 liquidity from launch; high reuse with the existing V3 family | Normalized volume and live liquidity still need a uniform measurement pass |
| 2 | LaunchHood V3 | 2,278 launches, 5,914 trades, 953 traders, 2,285 traded tokens, and 143.816 ETH in 24h; 2,819 launches in 7d | CREATE2 prediction and V3 liquidity from launch; no curve migration | Only 307 launch rows had more than one trade, so raw creation volume overstates opportunity |
| 3 | Clanker | Official chain-4663 labels show about 1.6k launches and 1.0k transactions in 24h, 4.7k launches in 7d | Native Uniswap v4 path; unlocks a reusable V4 adapter | Exact swaps, normalized volume, liquidity, and hook variants remain incomplete |
| 4 | Bankr/Doppler | A sampled latest-launch API window had 44 of 50 launches on Robinhood in 79m37s | Reusable Doppler/v4 execution; important smart-account and ERC-4337 coverage | Exact 24h swaps/volume/liquidity are unknown; outer signer is not necessarily the leader |
| 5 | trench.today | Official snapshot: $371,952.99 24h volume, 8,696 tokens, 36 lifetime graduates; proxy and sample chain transactions confirmed | High-value discovery target | Trade selectors/directions, curve math, sells, graduation, and stable implementation pins are unresolved |
| 6 | klik.finance | 512 launches in 24h and at least 4,395 in 7d from its V4 factory | Reuses the generic V4 foundation | Swap/volume/liquidity data and hook/dynamic-fee semantics are not yet proven |
| 7 | Flap | 10,852 launches, 65,937 buy/sell events, and 1,499 creators in 24h; 31,697 launches and 256,861 events in 7d | Strong pre-receipt portal events and CREATE2 identity | Upgradeable portal family, route/tax/vault variants, unknown normalized volume/liquidity/graduation |
| 8 | Pons | Current generation: 319 launches in 24h; 327 launches, 68 creators, and 192 atomic initial buys over its measured 7d lifetime | V3 at launch, deterministic prediction, no migration | Current factory source unverified; all-generation swaps/volume/liquidity and several semantics unresolved |
| 9 | hood.fun | 7,313 launches and 2,857 creators in 24h, but only 93 tokens with nonzero volume; 96.263 ETH indexed and 2 migrations | Curve observer can later reuse the curve-to-pool boundary | Extremely low active-token ratio and sparse migration evidence |
| 10 | leavehood.com | 208 launches and 34 direct curve swaps in 24h; 978 launches and 464 direct swaps in 7d | Direct curve calls are observable | Incomplete curve, upgrade, sell, and graduation semantics |

Ordering within a tier is an implementation priority, not a permanent quality score. Missing normalized economic metrics prevent a defensible single numeric score; pretending otherwise would reward whichever source exposes the most convenient counter.

## Observe-only and attribution exclusions

- **LaunchHood legacy curve:** only 17 launches and two graduations in the measured seven-day window; retain decoding fixtures but do not build a new execution path.
- **bags.fm:** the supplied activity is Solana-side. No chain-4663 contract, transaction, log, or code proof was established.
- **ape.store:** the supplied activity is Base-side. Base is 8453, not 4663.
- **long.xyz:** Base-oriented evidence does not prove Robinhood support.
- **Virtuals:** retain it as an attribution tag on Bankr/Doppler observations. Never dispatch, quote, or reconcile through a separate Virtuals adapter.

## Promotion gates

Every executable candidate must first have:

- exact chain ID 4663 enforcement and startup-pinned destination, implementation, factory, router, pool-manager, hook, and code hashes as applicable;
- a collision-free destination-plus-selector dispatch fixture and cross-adapter negative fixtures;
- complete buy/sell direction, quote asset, fee, slippage, value, tax, hook, and smart-account semantics for the allowlisted profile;
- local entry and exit quote/calldata construction with no candidate-time RPC;
- receipt-free prediction tests where the protocol claims deterministic token or pool identity;
- asynchronous reconciliation that proves the observed launch/market and the submitted route without becoming the initial decision gate;
- paper results showing zero identity, prediction, direction, and quote mismatches over a meaningful sample.

Candidate-specific blockers are documented in [PONS_FAMILY.md](./PONS_FAMILY.md), [FLAP_SH.md](./FLAP_SH.md), and [ECOSYSTEM_TIER2.md](./ECOSYSTEM_TIER2.md). Their machine-readable evidence is preserved in [PONS_FAMILY_EVIDENCE.json](./PONS_FAMILY_EVIDENCE.json), [FLAP_SH_EVIDENCE.json](./FLAP_SH_EVIDENCE.json), and [ECOSYSTEM_TIER2_EVIDENCE.json](./ECOSYSTEM_TIER2_EVIDENCE.json).
