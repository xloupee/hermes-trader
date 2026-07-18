# Pons launchpad-wide samples

Evidence time: 2026-07-17 UTC. Source commit:
`3829a7b2dccb2c651c85a920e19c2f705607ab6d`. Chain: Robinhood Chain
mainnet, chain ID `4663`.

## Result

The current-generation target is met: **10 of 10** selected canonical direct
launches passed all of the following checks:

- exact current factory, `launchToken` selector, successful receipt, and
  canonical `TokenLaunched` event identity;
- in-memory CREATE2 token prediction equals the receipt token;
- canonical Uniswap V3 CREATE2 pool prediction equals the receipt pool;
- strict receipt-end state reconstruction admits a fixed `0.001 ETH` paper
  entry;
- the independent 100 bps slippage minimum is present for entry and full exit;
- the full quoted token position exits completely in simulation; and
- the quote remains `execution_eligible=false` and `broadcast=false`.

All 10 token predictions matched, all 10 pool predictions matched, all 10
receipt quotes were available, and there were zero targeted prediction, quote,
receipt-event, or detector mismatches. The simulated immediate round trip was
`9801` bps for each sample, consistent with applying the 1% pool fee in both
directions; it is evidence of deterministic quote mechanics, not profitability.

This is paper evidence only. It grants no signing, transaction construction,
broadcast, canary, deployment, wallet, key, server, or production authority.

## Strict generation and envelope separation

One exact inclusive 750,000-L2-block scan was anchored from block `11,709,128`
(`0x50171c65...c0f`) through block `12,459,127`
(`0x24ffa780...584`) against
`https://rpc.mainnet.chain.robinhood.com`, concurrency 1.

| Generation/profile | Count | Readiness treatment |
| --- | ---: | --- |
| Current `TokenLaunched` receipts | 82 | Current-generation population |
| Canonical direct current envelopes | 28 | Eligible profile; 10 newest selected and fully replayed |
| Immutable reviewed EIP-7702 self-batch | 1 | Existing reviewed exception; not used to fill the 10-direct target |
| Rotating EIP-7702 self-batches | 51 | Fail closed; not quote eligible |
| Current receipts with ambiguous transaction lookup | 2 | Fail closed; no profile inferred |
| Legacy `TokenLaunched` receipts | 7,094 | Reported separately; never counted toward current readiness |

The legacy count is confirmation of activity only. Existing code deliberately
blocks legacy quote planning because its evidence profile is incomplete. The 51
rotating self-batches were recognized from receipt identity but were not
normalized into direct calls; only the already reviewed immutable wrapper is an
exception. The two ambiguous current transaction lookups remain unknown rather
than being guessed into a profile.

## Selected current samples

Each row is a canonical type-2 direct call to the pinned current factory with
selector `0x686399cb`, no authorization list, exactly one current-factory
`TokenLaunched` log, matching token/pool prediction, and an available strict
entry/slippage/full-exit quote.

| L2 block | Transaction | Token | Pool | Initial buy (wei) |
| ---: | --- | --- | --- | ---: |
| 12,253,335 | `0xc2e68aca57b79dcc2a937a41f264d043c9ab7f02d1005fd2b40feaf58809f45b` | `0x36a44e8d8ed52deab0202ad09788a750d3e6f73c` | `0x5f5026466ded4ccc3714684720bf0e07d20fabd0` | 300,000,000,000,000,000 |
| 12,271,575 | `0x960cff3cf3ccc6177000bf1f019012920a4ea815109455f9860f7c72481fd615` | `0x12e4c94680fa353deac197f0dc5930f4ac87ca05` | `0x3000330e9e1de955ab8032d9fefe2dcef3a16312` | 44,999,999,999,999,998 |
| 12,274,979 | `0x45c51676479c96d66e71c900f73b0ae24bb3bf020dc0a6c44e311d85e1c391a2` | `0x1b1318dd0226faca6fcee7fc58d5370f91303946` | `0x711f6bdb80a0b4e1381b35805648efee71456306` | 0 |
| 12,275,102 | `0xcfa5a0f4632e8b76abacd9f916aa3d55ae68f6b20c0f960a1bdac36fa2245b0a` | `0x7693a5f1fb975c0aa3bbbc6a26061c39c6a3622d` | `0xe38af1e7fea17703fd94e21d32a59540e80f528d` | 100,000,000,000,000,006 |
| 12,276,480 | `0x0ef49d8181b296a2b9529bdf23021dad80f8c136879cde3ff640573a4951c7b2` | `0x9f7710ccd2a265152e981d2b86036590e0a95280` | `0x226870ce0435b5d265cec7157856a8e386e020a2` | 0 |
| 12,356,072 | `0x274e45dc79f3b3074cc262d95d81034afdd85ac717c432e685094d7b0df0c1ff` | `0x8ecea3d0e648db646d824aa51eedeb16ac3d6878` | `0xd55246642dd114bc21db98c6f2261161a6158388` | 20,000,000,000,000,000 |
| 12,380,247 | `0x7d79edd0dd05ce413be7acd285395f5cb96f7057e7c972f780c3163eea96b4cf` | `0x3c33994637dcb8633e9415fcf7387ddb5b53f88b` | `0xcdc5ba3c80c547b73d120912f65ca1f3e4c2856f` | 99,500,000,000,000,000 |
| 12,401,299 | `0x906ca222b6114d3f137ead5b574ccba440dff896ce314d887c2e09bdb923f253` | `0x6a8422d915027f7aa18aef7b9c8c62789147de39` | `0xc06e24ace150c5863dc769402b047b3b7da29bd3` | 0 |
| 12,409,628 | `0xa3d2ed686c4b0c2d0a3af2cba706f67f66710aff1a8ace65aaecb22173f9236b` | `0x6a047fceef2c284cfa42448cbab8bf32ef37fae4` | `0x087c8adef5037490af6bc12b4119b2d0c7ffb6af` | 100,000,000,000,000,000 |
| 12,418,143 | `0xa6cf320396bf1fab613ea6e982d7635aeb1b2765f645b7e44ef97ed1739f80da` | `0xa15ffd07b08a2fe0b240be7e4b3ce1619bcf64b4` | `0xabf51208a8b8af2f9c0ac8dae86d26a597b99943` | 100,000,000,000,000,000 |

The machine artifact contains each full transaction-envelope commitment,
one-block ground-truth manifest, canonical receipt log, prediction comparison,
receipt-end pool state, entry output and minimum, full-exit output and minimum,
and execution blockers.

## Provenance and method

The production expected-pin document was kept separate from a newly collected
confirmed startup snapshot. Startup validation passed at L2 block `12,461,486`,
hash `0xe7ed9f56...4c32`: 41 pins and 83 logical RPC requests, with zero retries,
rate limits, server errors, or transport errors. The current factory runtime was
24,192 bytes with Keccak-256 `0x921a0d1b...1b5`, exactly matching the reviewed
pin.

The broad shared reconciler was not used as the final targeted artifact because
its registry-wide log query also collects other launchpads. Instead, each
selected Pons transaction was replayed through the unmodified reconciler in a
one-block, hash-anchored window with `--concurrency 1` and
`--max-ground-truth-blocks 1`. The pure in-memory Pons predictor was then run
against the same raw transaction calldata and compared with the receipt token
and pool. No candidate-time RPC is part of the predictor or quote calculation.

Important source SHA-256 commitments are embedded in `PONS_SAMPLES.json`,
including the adapter, predictor, receipt quoter, reconciler, expected pins, and
prior four-proof fixture. The machine file also binds every selected calldata
by byte length and SHA-256.

## Reproduction commands

Build only the existing read-only evidence binaries:

```sh
cargo build --release \
  --bin hermes-launchpad-pin-snapshot \
  --bin hermes-launchpad-paper \
  --bin hermes-launchpad-reconcile
```

Collect and validate the fresh startup snapshot:

```sh
./target/release/hermes-launchpad-pin-snapshot \
  --expected-pins config/launchpad-expected-pins.production.json \
  --confirmations 2 \
  --snapshot-output OBSERVED_STARTUP.json
```

The bounded population queries used exact factory/topic pairs:

```sh
curl -sS --max-time 60 \
  -H 'content-type: application/json' \
  --data '{"jsonrpc":"2.0","id":1,"method":"eth_getLogs","params":[{"fromBlock":"0xb2aac8","toBlock":"0xbe1c77","address":"0x0c37a24f5d23a486fa692d1500881d698b1f77a4","topics":["0xdb51ea9ad51ab453a65a4cb7e60c3cb378c9501bb002609f8f97778fb6c4235a"]}]}' \
  https://rpc.mainnet.chain.robinhood.com

curl -sS --max-time 60 \
  -H 'content-type: application/json' \
  --data '{"jsonrpc":"2.0","id":1,"method":"eth_getLogs","params":[{"fromBlock":"0xb2aac8","toBlock":"0xbe1c77","address":"0xa5aab3f0c6eeadf30ef1d3eb997108e976351feb","topics":["0xdb51ea9ad51ab453a65a4cb7e60c3cb378c9501bb002609f8f97778fb6c4235a"]}]}' \
  https://rpc.mainnet.chain.robinhood.com
```

Each selected block was reconciled separately with this shape (substitute the
adjacent block/hash pair):

```sh
./target/release/hermes-launchpad-reconcile \
  --acquisition replay \
  --input OBSERVER_CAPABILITIES.jsonl \
  --expected-pins config/launchpad-expected-pins.production.json \
  --observed-startup-snapshot OBSERVED_STARTUP.json \
  --concurrency 1 \
  --paper-amount-in-wei 1000000000000000 \
  --paper-max-amount-in-wei 10000000000000000 \
  --paper-slippage-bps 100 \
  --ground-truth-start-head BLOCK_MINUS_ONE \
  --ground-truth-start-hash START_HASH \
  --ground-truth-cutoff-head BLOCK \
  --ground-truth-cutoff-hash BLOCK_HASH \
  --ground-truth-confirmations 2 \
  --max-ground-truth-blocks 1
```

Local integrity checks:

```sh
jq empty research/launchpads/samples/pons/PONS_SAMPLES.json
jq -e '.counts.current_samples == 10 and
       .counts.current_quotes_available == 10 and
       .counts.current_prediction_token_matches == 10 and
       .counts.current_prediction_pool_matches == 10 and
       .counts.prediction_mismatches == 0 and
       .counts.quote_mismatches == 0 and
       .counts.legacy_counted_toward_current_readiness == 0' \
  research/launchpads/samples/pons/PONS_SAMPLES.json
git diff --check
git diff --name-only --diff-filter=ACMRTUXB
```

## Assumptions, risks, and unresolved questions

- The fixed cutoff is a historical snapshot; later chain activity is outside
  this artifact.
- Receipt event identity proves current versus legacy generation, but an outer
  wrapper is quote eligible only when its complete envelope is independently
  reviewed. Rotating wrappers therefore remain blocked.
- Two current log identities encountered transaction-lookup ambiguity after
  the population scan. They are counted as unknown/fail-closed and were not
  used as samples.
- The 10 direct samples prove the existing prediction and quote engines across
  zero and nonzero launch-time buys. They do not prove live execution safety,
  post-launch market depth, later exit availability, or economic profitability.
- No shared implementation gap was needed to achieve the requested evidence.
  If broader rotating-wrapper coverage is desired, it requires a separate
  envelope review and integration-owner decision; this evidence branch must
  not infer it.

## Integration

The integration owner can cherry-pick the single evidence commit from
`codex/samples-pons`. Only
`hermes-feed/research/launchpads/samples/pons/**` is changed. Do not merge or
copy any temporary build/replay files; all authoritative results are contained
in `PONS_SAMPLES.json` and this report.
