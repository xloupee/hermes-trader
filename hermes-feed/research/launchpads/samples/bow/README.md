# Bow bounded launchpad-wide samples

## Result

The bounded 750,000-L2-block scan found 67 canonical Bow `Launched` events from block 11,709,264 through 12,459,263. Twenty direct-factory receipts were replayed through the existing paper quote implementation and admitted: 10 `zero_initial_buy` and 10 `payable_initial_buy`. No wallet filter was used.

Every admitted sample produced a complete fixed 0.001 WETH paper entry, a 1% slippage floor, and an immediate full-position exit. Every simulated round trip returned `0x2649` (9801) bps. All rows remain paper-only: `execution_eligible=false`, `broadcast=false`, with token restriction and runtime gates unsatisfied.

The scan counted 60 direct-factory envelopes (14 zero-value and 46 payable) and seven wrapped envelopes. Only the selected 20 direct receipts are claimed quote-eligible; the other 40 direct candidates were not replayed after both 10-sample targets were met. The seven wrapped transactions were not generalized into a supported profile.

## Bounded provenance

- Source SHA: `3829a7b2dccb2c651c85a920e19c2f705607ab6d`
- Branch: `codex/samples-bow`
- Chain ID: `4663`
- Public RPC: `https://rpc.mainnet.chain.robinhood.com`
- Blockscout: `https://robinhoodchain.blockscout.com`
- Concurrency: `1`
- Inclusive range: `11709264` (`0x7d83e9b3c5e55a140f2073a55bd56c84d6359100c1030a59708ddc38b0cb27c9`) through `12459263` (`0xfe88815533e15801060f5c822e9446d0008d63267c1c46b87db1f3d65e1fc328`)
- Bow factory: `0xc70e510e14710ea535cab7b2414860af63feab79`
- Observed factory runtime hash: `0x8d56cbcdf72dbf04ed8170d55878cc894997ccc54c2ab0aec782274eb7fe7a14`
- Launch selector: `0xf6efccd9`
- Canonical event topic: `0xec774f0683e9ac48e8d835f412f9f877a8a5dee9af3170d78cf3ef33149d15e7`

The machine artifact binds each admitted transaction to its block hash, transaction index, transaction value, deployer, token, pool, canonical receipt-event indices, profile, and entry/exit/slippage outputs.

## Requested strict-envelope miss

Transaction `0x6460c0afc4cbdac9e5e5b62db5eb982a92d4affc7051ccf89daa1e5df332f100` succeeded on chain but is not an admitted direct Bow envelope. Its top-level type-4 call goes from `0xdda62e...` to Multicall3 `0xca11bd...` using `aggregate3Value`, while the canonical Bow event identifies deployer `0x4009db...`. The current strict quote path therefore returns `TransactionEnvelope` before state replay because both the top-level destination and sender disagree with the direct-factory envelope.

Blockscout decodes 31 outer calls. The nested Bow launch has value zero and is factually a `zero_initial_buy` launch, but the receipt then contains 30 separate buyer swaps. That is incompatible with both admitted shapes: zero-buy permits zero swaps, and payable-buy permits one independently reconstructed deployer swap. The transaction remains a scored unsupported Multicall3/smart-account batch miss. It has no quote eligibility and no fabricated entry, exit, or slippage result.

## Commands and checks

```sh
git switch -c codex/samples-bow 3829a7b2dccb2c651c85a920e19c2f705607ab6d
cast chain-id --rpc-url https://rpc.mainnet.chain.robinhood.com
cast block-number --rpc-url https://rpc.mainnet.chain.robinhood.com
cast block 11709264 --json --rpc-url https://rpc.mainnet.chain.robinhood.com
cast block 12459263 --json --rpc-url https://rpc.mainnet.chain.robinhood.com
cast codehash 0xc70e510e14710ea535cab7b2414860af63feab79 --rpc-url https://rpc.mainnet.chain.robinhood.com
curl -sS --get https://robinhoodchain.blockscout.com/api \
  --data-urlencode module=logs --data-urlencode action=getLogs \
  --data-urlencode fromBlock=11709264 --data-urlencode toBlock=12459263 \
  --data-urlencode address=0xc70e510e14710ea535cab7b2414860af63feab79 \
  --data-urlencode topic0=0xec774f0683e9ac48e8d835f412f9f877a8a5dee9af3170d78cf3ef33149d15e7 \
  --data-urlencode page=1 --data-urlencode offset=1000
CARGO_TARGET_DIR="$(mktemp -d /tmp/bow-samples-build.XXXXXX)" \
  cargo build --release --bin hermes-launchpad-v3-paper-quote
hermes-launchpad-v3-paper-quote --tx-hash HASH --launchpad bow
cast tx HASH --json --rpc-url https://rpc.mainnet.chain.robinhood.com
cast receipt HASH --json --rpc-url https://rpc.mainnet.chain.robinhood.com
curl -sS https://robinhoodchain.blockscout.com/api/v2/transactions/HASH
jq empty bow_samples_2026-07-17.json
jq -e '(.samples|length)==20 and ([.samples[].profile]|group_by(.)|map(length)|sort)==[10,10]' bow_samples_2026-07-17.json
```

## Assumptions, risks, and integration

- The Blockscout legacy log endpoint returned all 67 canonical events in one page (`offset=1000`); the count is below the page cap. Transactions and receipts used for admission were independently fetched from the public RPC.
- Profile counts over the whole scan use the strict top-level direct-envelope distinction. Nested wrapped launch semantics are not generalized from the one diagnosed miss.
- Receipt-end quotes are deterministic paper outcomes, not market-execution or restriction-clearance claims.
- No shared implementation gap was changed. If wrapped smart-account batches are to be supported, the integration owner needs a separate reviewed profile that binds nested call value/sender and isolates the launch-local swap sequence; the present evidence must not be used to relax the direct envelope.
- Integrate by cherry-picking this branch commit. Do not merge it as an execution enablement; it contains research evidence only.
