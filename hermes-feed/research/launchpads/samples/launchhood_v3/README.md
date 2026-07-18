# LaunchHood V3 bounded paper samples (2026-07-17)

This launchpad-wide, wallet-independent scan reached the requested target: **10 independently confirmed `embedded_initial_buy` observations**. It queried 100,000 recent Robinhood Chain L2 blocks, below the 500,000-block cap, and stopped after the second 50,000-block chunk crossed the target.

## Provenance and bounded scan

- Source commit: `3829a7b2dccb2c651c85a920e19c2f705607ab6d`
- Source tree: `049e859ad896ffdde24444ece6ef6e417a12c94e`
- Public read-only RPC: `https://rpc.mainnet.chain.robinhood.com` (chain `4663`)
- Queried range: blocks `12,359,274` through `12,459,273`, inclusive
- Range-start hash: `0x34e34c8b34ca8ed20d4ffb7e3419050b06db58078b66562528d0c4bc1ab047ef`
- Anchor hash: `0x59c36d4866806364928fef1512c9b9cdbac629aeceabe500686e4f28ce87486c`
- Canonical factory-event hits: `21`; selected: the `10` most recent; unselected after reaching the target: `11`
- RPC and quote concurrency: `1`
- Release paper-quoter SHA-256: `45eca76fdc54b81c8979f6f40b3d152489bd2ce80d353044dd63e8477950c8dc`

The scan filter was the exact LaunchHood V3 factory `0x62b33a039d289cbda50ebeb72fe4261449e61bcf` and canonical `TokenLaunched(address,address,address,address,uint256,uint256,uint256,uint256,uint256,uint256)` topic `0x235e34a4e0e6a401dae6851f6fab4a919a1fdd0ae0073ac2fc4d1d4a87e548e5`. Every selected transaction used pinned launch selector `0x4110a41c`; no wallet filter was used.

## Identity and quote result

At the anchor block, the exact checked-in runtime hashes matched for the LaunchHood factory, its 6,821-byte token implementation, canonical V3 factory, WETH, and SwapRouter02. Each selected successful receipt had exactly one canonical factory event, one canonical pool creation, one mint, and one embedded swap. The receipt quoter independently matched event token and pool identities, transaction value to declared `initialBuyAmount`, and the reconstructed embedded swap to declared `initialBuyTokens`.

All 10 samples were quote-eligible. Each generated:

- an independent fixed `0.001 WETH` entry (not the launcher buy amount);
- a `100 bps` entry minimum;
- a full-position exit with a separate `100 bps` minimum;
- an immediate simulated round-trip return of `9,801 bps`.

These are receipt-end paper calculations, not realized profit or loss. Every sample remains `execution_eligible=false` and `broadcast=false`; no wallet, signer, transaction construction, deployment, SSH, or server mutation occurred.

## Counters

| Counter | Value |
|---|---:|
| confirmed `embedded_initial_buy` | 10 |
| quote eligible / entry plans / full exits | 10 / 10 / 10 |
| pin mismatches | 0 |
| RPC ambiguities | 0 |
| receipt or canonical-event cardinality failures | 0 |
| factory / selector / profile mismatches | 0 / 0 / 0 |
| token / pool / initial-buy mismatches | 0 / 0 / 0 |
| embedded-swap / quote mismatches | 0 / 0 |
| entry / full-exit plan misses | 0 / 0 |

The machine-readable sample rows, exact block and receipt identities, token/pool pairs, entry/exit amounts, pins, range hashes, and full counters are in `launchhood_v3_samples_2026-07-17.json`.

## Reproduction outline

1. At source commit `3829a7b...`, build `hermes-launchpad-v3-paper-quote` with a temporary `CARGO_TARGET_DIR` outside the repository.
2. Read `eth_chainId` and anchor `eth_getBlockByNumber` from the public RPC.
3. Verify the five exact runtime code hashes at the anchor block.
4. Call `eth_getLogs` sequentially in 50,000-block chunks using the factory and topic above; stop when 10 candidates are available or 500,000 blocks have been queried.
5. For the 10 most recent candidates, fetch transaction and receipt sequentially and run:

   ```sh
   hermes-launchpad-v3-paper-quote \
     --tx-hash <HASH> \
     --launchpad launch-hood-v3 \
     --amount-in-wei 1000000000000000 \
     --max-amount-in-wei 10000000000000000 \
     --slippage-bps 100 \
     --rpc-url https://rpc.mainnet.chain.robinhood.com
   ```

6. Re-read the range boundary hashes, validate the JSON with `jq`, and reject any pin, chain, receipt, event, identity, embedded-swap, or quote ambiguity.

## Integration

Cherry-pick this branch commit or copy only `hermes-feed/research/launchpads/samples/launchhood_v3/`. Do not interpret these samples as canary or execution authorization; they are additive paper evidence only.
