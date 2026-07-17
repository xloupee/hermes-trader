# StonksLauncherV3 receipt-confirmed paper quote evidence

## Verdict

The exact `stonks_v3_direct_launch` WETH profile now has a receipt-confirmed,
independent paper quote. It remains unavailable at candidate time and remains
outside the promotion-readiness set. The quote, finalized entry sizing, full
position exit, take-profit, stop-loss, and max-hold plan are all non-executable:
`paper_evidence_ready=false`, `authorizes_canary=false`,
`execution_eligible=false`, and `broadcast=false`.

The implementation parent is
`74c1062678591ea5e55ec9c2140e0e22a9d5551c`.

## Independent quote construction

The receipt-block observer remains the authority for the exact direct wrapper,
EOA leader, launcher calldata, dependency runtimes/getters, USDG implementation
slot, DN404 asset/mirror linkage, canonical V3 pool, initialization, and all
eleven ordered positions. The quoter accepts only that complete observation.
It reconstructs the receipt-end pool with:

- token0: `0x01c1f35f9463c39ea9169334a64c0ed3a340ac85`;
- token1 / quote asset: WETH
  `0x0bd7d308f8e1639fab988df18a8011f41eacad73`;
- pool fee: `10000` ppm (1%);
- tick spacing: `200`;
- receipt-end tick/sqrt price: `-197400` /
  `0x363db22b79374d1d73fc0`;
- active liquidity: `0xd337e874824d30601b`;
- the exact ten dense positions ending at `-144400` and exact tail
  `[-144400,887200]`, including every liquidity and token amount word.

The fixed independent policy spends `0.001 WETH`, caps input at `0.01 WETH`,
and applies `100` bps slippage. These are module invariants: the public quoter
accepts no policy argument, reconciliation cannot substitute shared CLI flags,
and replay derives the constants rather than trusting serialized amount or
slippage fields. It does not reuse a launcher amount: the direct launch has no
embedded buy. The generated entry output is
`0x4df6af20a04e32bb7276`, with minimum receive
`0x4d2f18e56b809dbeb150`. The immediate full-position exit returns
`0x37b7077f59f6e` WETH with minimum receive `0x372866247a583`.
The simulated return is `9801` bps, exactly the two sequential 1% pool fees in
this no-tick-crossing round trip.

The quote carries a canonical digest of the exact receipt observer record. The
finalizer requires the separately emitted observation to match that digest and
the quote's leader/creator, launcher, asset, mirror, pool, block, transaction,
and every ordered receipt log position. It independently replays every position
and both swap legs before it emits a paper plan. The plan uses 2000 bps take
profit, 1000 bps stop loss, and
a 300-second maximum hold. The future trigger quote remains explicitly
unevaluated. A raw-frame transaction-inventory sequence can bind the finalized
record only after receipt ground truth identifies Stonks; it is not converted
into a candidate-time observer claim or prediction.

## Real first-swap differential

The first pool `Swap` after the proof launch is transaction
`0x2865fdf3440838b22832d5d07701b7e66a391c28f82ae1e20bd272971982206e`
at L2 block `12038063`, log index `30`. Its exact input is `0.005 WETH`.
The differential fixture contains the raw pool emitter, topics, and data. The
production V3 event decoder reconstructs the indexed sender/recipient and
signed amounts before the test uses those decoded amounts. Starting only from
the independently reconstructed launch receipt state, the local V3 quote
matches all emitted swap fields exactly:

- token output: `1805561610898109480341640`;
- sqrt price after: `0x3792ba0f1a84d40313693`;
- liquidity after: `0xd337e874824d30601b`;
- tick after: `-196915`.

The acquisition is public read-only `eth_getLogs`; no signer, wallet, key,
broadcast, server, deployment, or expected-pin mutation was used.

## Runtime and negative coverage

The hermetic proof uses the concrete `NoxaRpcClient` against a loopback JSON-RPC
server. It crosses historical code, storage, getter, canonical-block, dynamic
asset/mirror, and pool-linkage reads before observation and quote. Separate
tests cover the exact raw Nitro decode boundary, reconciliation quote dispatch,
serialized quote replay, and finalizer output.

Fail-closed tests reject unsafe sizing/slippage, amount rounding changes,
position order, liquidity and amount drift, token orientation, fee changes,
tick-crossing/state changes, dependency-pin drift, non-canonical pool identity,
leader/creator/mirror drift, receipt-log index drift, incomplete liquidity,
duplicate quote records, absent raw inventory, finalized-record tampering, and
coherently regenerated `0.005 WETH`, `500` bps, or max-cap policy drift.

Fixture SHA-256 values:

- exact raw Nitro record:
  `8bfa5246a53ed9d52b02b939dfec1dd372e3fa4396f115d6daa1b535fee520e3`;
- selected transaction/receipt/block proof:
  `2b65401b38e7f61b32025a8e714f30a4a0aea22fe5a4d478a3a23872ef3d779c`;
- independent paper quote:
  `5a01f176480c6e8e617de9e3c4eb2a21a7e5c33379287a552fb3d3ed2d02de52`;
- first-swap differential:
  `979175df35050b22918447779a15e08c7eacfd4cbe03fdd7573c8fe4a34799ed`;
- hermetic concrete RPC transcript:
  `c23ebcd4b1af5c474b6ea9c999cfd766ab7753ae740c255f3f2bf77f06733819`.

## Remaining limitation

This evidence proves receipt-confirmed quote arithmetic for one exact WETH
profile and one real first swap. It does not provide a candidate-time token or
pool prediction, does not establish a meaningful readiness sample, does not
support USDG or `launchAndBuy`, and does not authorize a canary or execution.
