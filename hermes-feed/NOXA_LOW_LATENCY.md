# NOXA low-latency research and architecture

Research snapshot: 2026-07-12 UTC. This document separates facts verified on
Robinhood Chain from hypotheses that still require testnet measurement.
Machine-readable evidence retained from the research run is in
[`oracle/noxa-research-snapshot.json`](oracle/noxa-research-snapshot.json).

## Outcome

Hermes should treat NOXA as a sparse Uniswap V3 launch race, not as a V2 copy
trade and not as a Solana-style pending shred stream.

The target lean data path is:

```text
NOXA launchToken transaction at Robinhood L1 context B
  -> post-execution Nitro feed detection
  -> successful receipt and exact TokenLaunched verification
  -> parse the entire receipt in logIndex order
  -> PoolCreated + Initialize + Mint + optional post-event Swap
  -> exact local sparse-V3 quote
  -> restriction policy using Ethereum L1 height
  -> pre-wrapped WETH + prior SwapRouter02 approval
  -> prebuilt and pre-signed single-hop transaction
  -> predictive parent-L1 trigger (working hypothesis, calibrated and risky)
     OR conditional L1 boundary retry (safe fallback)
     OR post-execution feed transition (last-resort fallback)
```

The current `hermes-noxa` binary implements the path through verified receipt
hydration, exact-input quote, token restriction snapshot, paper decision, and
prepared router calldata. Transaction construction/signing exists as a strict
library primitive, but the CLI deliberately has no key loading, signing, send,
or retry loop. The existing V2 observer, cache, and live paper processes are
unchanged.

## Current chain state

The canonical launch factory is
`0xD9eC2db5f3D1b236843925949fe5bd8a3836FCcB`. At pinned L2 block 8,181,686
(L1 25,519,674), `launchEnabled()` was still `false`. The owner disabled
launching in transaction
[`0xdf03…90c8`](https://robinhoodchain.blockscout.com/tx/0xdf03d9cd279a3fceb940ef1665c4367d0cf6e64a2bfc9738e44e20de3ddc90c8),
nine seconds after the final successful launch on 2026-07-11.

An exhaustive non-overlapping event scan found:

- factory deployment at L2 block 61,688;
- 60,142 ABI-valid `TokenLaunched` events from the canonical factory;
- final successful launch at L2 block 6,880,646;
- zero successful launch events afterward through the research snapshot.

Bots continue sending `launchToken` calls while the factory is paused. Live
Hermes observation confirmed that these calls have status `0x0` and no logs.
Therefore a factory call is never a launch signal until its successful receipt
contains the exact canonical event.

The observer remains useful while paused: it classifies reverted launch spam
and keeps the measured feed/RPC path warm. It reads enablement at startup; it
does not monitor owner enable/disable calls continuously.

## Pinned contracts and configuration

Official NOXA and Uniswap deployments, then confirmed on-chain:

| Component | Address |
|---|---|
| NOXA LaunchFactory | `0xD9eC2db5f3D1b236843925949fe5bd8a3836FCcB` |
| NOXA LaunchLocker | `0x7F03effbd7ceB22A3f80Dd468f67eF27826acD85` |
| WETH | `0x0Bd7D308f8E1639FAb988df18A8011f41EAcAD73` |
| Uniswap V3 factory | `0x1f7d7550b1b028f7571e69a784071f0205fd2efa` |
| NonfungiblePositionManager | `0x73991a25c818bf1f1128deaab1492d45638de0d3` |
| QuoterV2 | `0x33e885ed0ec9bf04ecfb19341582aadcb4c8a9e7` |
| SwapRouter02 | `0xcaf681a66d020601342297493863e78c959e5cb2` |
| Universal Router | `0x8876789976decbfcbbbe364623c63652db8c0904` |

The factory is a direct, non-proxy contract. Its pinned runtime is 22,811 bytes
with Keccak-256
`0xadcfca67f5d7df9f26974a07be2b5d83894765e6e5e9b9f0a232223f25c795e6`.
Hermes status refuses to call the runtime canonical when that hash changes.

Current launch config 0 is:

- WETH pair token;
- Uniswap DEX ID 0;
- 1% pool fee (`10000`) and tick spacing 200;
- supply `1,000,000,000 * 10^18`;
- current launched-token max wallet `2%` and max transaction equal to supply,
  both read from each token rather than trusted as constants;
- 366 Ethereum L1 blocks of restrictions;
- current launch fee `0.0005 ETH`.

The factory ABI is unverified on Blockscout. The ABI pinned in Hermes is
supported by the official NOXA frontend, deployed selectors, historical input,
and successful receipt/event decoding. Runtime code must not fetch the frontend
bundle.

## Launch and restriction semantics

The launch call selector is `0x686399cb`:

```solidity
launchToken(
  (string,string,string,string,(string,string,string,string,string),address),
  uint256 launchConfigId,
  uint256 dexId,
  bytes32 salt
)
```

There is no initial-buy calldata field:

```text
initialBuyAmount = tx.value - launchFee
```

The canonical event topic is
`0xdb51ea9ad51ab453a65a4cb7e60c3cb378c9501bb002609f8f97778fb6c4235a`.
Its indexed fields are token, deployer, and DEX factory. Its seven data words
are pair token, pool, DEX ID, launch config ID, position ID, restriction-end
block, and initial buy.

Public buys are blocked in launch L1 block `B`. Restrictions are active while:

```text
B < current Ethereum L1 block <= restrictionEndBlock
```

During that interval the resulting recipient balance must remain at or below the max
wallet, and cumulative bought output per `tx.origin` in one transaction is
capped at floor(`maxTxLimit * 110 / 100`). Robinhood's L2 block number must
never be substituted for the L1 height. Hermes has unit tests for every exact
boundary and overflow path.

## Why the full receipt matters

`TokenLaunched` is not the last launch log. In real receipts the order is:

```text
PoolCreated -> Initialize -> Mint -> TokenLaunched -> transfers -> Swap
```

The final `Swap` is the launcher's optional initial buy and is the authoritative
post-launch price, tick, and active liquidity. Stopping at `TokenLaunched`
produces a stale quote.

Hermes reconstructs only the state exact-input swap math needs:

- current sqrt price, tick, and active liquidity;
- fee and tick spacing;
- initialized-tick bitmap;
- liquidity gross/net at minted bounds.

It does not cache fee growth, observations, or every V3 pool. That keeps the
working set to one new launch pool.

The local Rust simulation was differentially validated against the first
public swap after real launch transaction
[`0xc629…e418`](https://robinhoodchain.blockscout.com/tx/0xc62997c2607d579233b552fad71faae7e392a4c13bc92b9d20c57425b9ffe418).
The oracle is swap transaction
[`0x455d…6a55`](https://robinhoodchain.blockscout.com/tx/0x455d4d7921a94e1388ad03f2024ea10d0de41166f5000b7ff62efa6cf51a6a55),
at L2 block `0x68fdbc` and L1 `0x1853bf5` (two L1 heights after the launch).
For an exact 0.012 WETH input Hermes reproduces all three observable results
exactly:

- amount out `0x6b0c664736ce5a3db06f7`;
- final sqrt price `0x657f421942f1ecfd3c158b1a091b`;
- final tick `203314`.

No Quoter or RPC call occurs in quote math.

On the Falkenstein host, the reviewed release binary repeated this hydrated
single-step quote 100,000 times at an average of 3,494 ns per quote. That is a
local-math microbenchmark, not end-to-end launch-to-submission latency.

Hermes' own `backfill` command then scanned the factory address/topic history
from L2 block 61,688 through 6,900,000: all 60,142 event payloads ABI-decoded and
zero were malformed. This proves event-shape compatibility, not full receipt or
token canonicality for every historical event; `inspect` performs the deeper
receipt/token checks. The public RPC rate-limited the first attempt; the client
now uses bounded exponential backoff for cold historical reads.

## Trigger research: fastest versus safest

### Nitro feed is not pending order flow

Nitro's TransactionStreamer executes a message before publishing the feed
soft-confirmation. A feed message has a `MessageResult`/block hash already. The
feed remains valuable for launch detection, sequence integrity, reconciliation,
and a safe fallback, but seeing the first `B+1` feed header is too late to land
in the first `B+1` L2 block.

### Fastest primary: parent Ethereum `newHeads`

The best public predictive signal found in the current documentation and smoke
tests is the parent Ethereum head. The working low-latency hypothesis is:

1. prepare and sign during launch context B;
2. observe parent Ethereum head B+1 from a co-located low-latency provider;
3. wait an empirically calibrated delay;
4. send the standard raw transaction directly to the Robinhood sequencer.

Provider arrival can lead or lag sequencer adoption, so this is not a universal
ordering guarantee. Send too early and the transaction executes under B and
reverts. It must remain paper/testnet-only until a measured delay distribution
and explicit loss budget exist.

`hermes-noxa calibrate-boundary` measures parent-head arrival to the first
post-execution Robinhood feed message for the same L1 number. A two-sample
Falkenstein/public-provider smoke test measured 7.11 s and 8.32 s. Those are not
a production sample and do not by themselves identify the safe send instant.

### Safe fallback: Nitro conditional transaction

Robinhood's direct sequencer exposes `eth_sendRawTransactionConditional`.
Hermes builds conditions with:

```json
{
  "knownAccounts": {},
  "blockNumberMin": "B+1",
  "blockNumberMax": "B+k",
  "timestampMax": "short deadline"
}
```

Nitro interprets these block bounds as L1 block numbers. An explicit JSON-RPC
`-32003` containing `BlockNumberMin condition not met` rejects before queueing,
so the exact same raw bytes may be retried without consuming nonce or gas.
Ambiguous transport failures must be reconciled by transaction hash; never sign
a different replacement blindly.

This is reject-not-hold. The check uses an already executed L2 head, and Nitro's
default required state age can delay acceptance further. It can beat feed
propagation slightly and is safe against launch-block execution, but it cannot
guarantee the first `B+1` L2 block.

No Timeboost/express-lane or preconfirmation/hold API was found in Robinhood's
current public documentation. Future-nonce queue tricks are undocumented and
intentionally excluded.

## Transaction construction

Robinhood documents first-come, first-served ordering; priority fees do not
bypass earlier arrivals. Network placement and connection reuse dominate.

The target baseline to benchmark is:

- dedicated wallet and exclusive nonce ownership;
- WETH wrapped before a launch;
- SwapRouter02 allowance approved before a launch;
- one direct `exactInputSingle` (`0x04e45aaf`) or `exactOutputSingle`
  (`0x5023b4df`) call;
- transaction value zero;
- exact local amount-out minimum and sqrt-price limit;
- transaction pre-signed while L1 is still B (library primitive only today);
- persistent pre-warmed HTTP/2 connections.

Exact output is attractive when sizing directly to the remaining max-wallet
allowance, but exact-output local quote math is not implemented yet. Universal
Router is Uniswap's general preferred entry point; direct SwapRouter02 remains
a project-specific latency hypothesis that must win an end-to-end benchmark.

The mainnet sequencer DNS currently maps to AWS us-east-2 (Ohio). Fresh TLS from
Falkenstein measured roughly 345–368 ms, while a reused HTTP/2 connection still
took roughly 112–115 ms. Feed-region winner and submission-region winner are
separate decisions. A mainnet trader should be benchmarked in us-east-2 before
deployment. Robinhood's full-node requirement starts at 64 GB RAM and several
TB of NVMe, so the current 4 GB host should use the direct feed plus a read RPC,
not a full node.

## Implemented commands

```bash
# Current enablement, launch fee, L1/L2 pin, and bytecode hash
hermes-noxa status

# Reconstruct a real receipt and quote it locally
hermes-noxa inspect --tx-hash 0xc62997c2607d579233b552fad71faae7e392a4c13bc92b9d20c57425b9ffe418

# Factory-address/topic event decoder/backfill
hermes-noxa backfill --from-l2-block 6880646 --to-l2-block 6880646

# Post-execution feed observer plus bounded async receipt verification
hermes-noxa observe --run-seconds 60

# Parent-head to Robinhood-feed boundary measurement
hermes-noxa calibrate-boundary \
  --l1-ws-url wss://YOUR_ETHEREUM_PROVIDER \
  --samples 100 \
  --run-seconds 7200
```

`observe` distinguishes unverified factory calls, reverted launch attempts, and
fully verified launches. On a verified launch it reconstructs V3 state, strictly
matches the reported initial buy, quotes locally, reads the token's own limits
at the receipt block, and emits a paper router/trigger plan. A policy decision
requires a recipient so its exact balance can be read; otherwise policy is
reported as not evaluated. It never loads a key, signs, or submits.
Every ten seconds it also emits cumulative sequence gaps, missing or reordered
messages, throughput, reconnects, verifier capacity, and shared RPC
request/retry/rate-limit/server/transport counters. The bounded measurement
runner combines these journals with parent-head/feed samples and 30-second
`launchEnabled()` snapshots. The persistent observer independently continues
the same 30-second factory watch after a bounded measurement finishes.

## Testnet-only orchestration

Robinhood testnet is pinned to chain ID `46630`, public RPC
`https://rpc.testnet.chain.robinhood.com`, feed
`wss://feed.testnet.chain.robinhood.com`, and direct sequencer
`https://sequencer.testnet.chain.robinhood.com`. The orchestration library now
provides a single-owner nonce lease, signer-bound pre-signed self-transfer
canary, conservative conditional retry decisions, hash reconciliation states,
and latched trade/gas/exposure/session-loss limits.

`testnet-submit-canary` accepts only externally signed bytes and defaults to a
read-only validation pass. It recovers the signer, requires a data-free
self-transfer on chain 46630, matches the pending nonce, enforces independent
value and worst-case gas caps, and checks funding before an explicit
`--broadcast`. It records each network attempt and receipt latency with a
stable source label for Falkenstein/Ohio comparison. Ambiguous transport or
RPC outcomes retain the nonce and reconcile only by the signed hash; the
command never reads or stores a private key. See `NOXA_CANARY_RUNBOOK.md`.

The read-only preflight checks the pending nonce, native gas balance,
pre-wrapped token balance, exact router allowance, and deployed router code:

```bash
hermes-noxa testnet-preflight \
  --account 0xTHROWAWAY_ACCOUNT \
  --wrapped-native 0xTESTNET_WRAPPED_NATIVE \
  --router 0xTESTNET_ROUTER \
  --amount-in 1000000000000000
```

NOXA does not currently publish a Robinhood testnet launch deployment in its
contract table. Therefore the first canary is intentionally a capped testnet
self-transfer that proves nonce/sign/submit/reconcile behavior without assuming
mainnet contract addresses. A swap canary remains fail-closed until testnet
wrapped-native and router addresses are independently verified.

## Verification snapshot

The final optimized offline suite passes 86 tests: 81 library tests and five
main-binary tests. It covers canonical ABI encoding, hot-header rejection,
receipt chronology and initial-buy correlation, every restriction boundary,
RPC retry/strict word parsing, signer-bound router construction, sequencer hash
reconciliation, zero-liquidity V3 gaps in both directions, multiword tick
traversal, and failure-atomic pool hydration.

The historical `inspect` replay also passed against the official RPC with all
eight launched-token views checked at exact L2 block 6,880,646. A five-second
live feed smoke test at L2 head 8,181,686 decoded contiguous paused-factory spam,
kept it suppressed during catch-up warmup, and exited with the verifier/output
queues drained. No wallet, signer, transaction submission, service restart, or
deployment was used.

## Promotion gates

The current implementation is a paper trader, not an authorized mainnet sender.
Before enabling any value-bearing path:

1. Hydrate a broad random sample from the 60,142 ABI-valid factory events,
   including both token orientations and zero/non-zero initial buys, and retain
   a reproducible sample manifest.
2. Differential-test multi-tick exact-input and exact-output results against
   QuoterV2 at eligible pinned blocks.
3. Run at least 24 hours of parent-L1/feed boundary calibration from us-east-2.
4. Test conditional retry semantics and required-state-age behavior on testnet
   with a dedicated throwaway funded wallet.
5. Benchmark direct exact-input, exact-output, and any custom executor by
   resulting feed position, not RPC acknowledgement.
6. Replace the current post-receipt restriction RPC reads with proven cached or
   deterministic state for any predictive sender, then prove no candidate-time
   RPC, allocation, JSON serialization, lock, or signing remains in its submit
   path.
7. Require explicit approval for a tiny-value mainnet canary, dedicated wallet,
   and deployment. Keep the existing V2 service untouched.

Exact CREATE2 token-address prediction remains a research item. The caller salt
is passed directly to CREATE2, but the init-code hash has not yet been broadly
reconstructed and validated. Receipt-based token discovery is authoritative.

## Primary sources

- [NOXA launchpad integration and restrictions](https://docs.noxa.fi/integrations/launchpad/)
- [NOXA Robinhood contract addresses](https://docs.noxa.fi/contracts/noxa-fun/)
- [Robinhood connection and direct sequencer endpoints](https://docs.robinhood.com/chain/connecting/)
- [Robinhood first-come, first-served ordering](https://docs.robinhood.com/chain/)
- [Robinhood full-node requirements](https://docs.robinhood.com/chain/run-a-full-node/)
- [Uniswap V3 Robinhood deployments](https://developers.uniswap.org/docs/protocols/v3/deployments/v3-robinhood-chain-deployments)
- [Nitro conditional option fields](https://github.com/OffchainLabs/go-ethereum/blob/f3a977ddf30b138da2fe673ac5cbff2bc6dd4c88/arbitrum_types/txoptions.go)
- [Nitro conditional checks](https://github.com/OffchainLabs/go-ethereum/blob/f3a977ddf30b138da2fe673ac5cbff2bc6dd4c88/arbitrum/conditionaltx.go)
- [Pinned Nitro transaction pre-checker](https://github.com/OffchainLabs/nitro/blob/3599acae1ad2fab4059fc46453c9cd3294126641/execution/gethexec/tx_pre_checker.go)
- [Pinned Nitro sequencer](https://github.com/OffchainLabs/nitro/blob/3599acae1ad2fab4059fc46453c9cd3294126641/execution/gethexec/sequencer.go)
