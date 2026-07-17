# Bankr/Doppler bounded launchpad-wide paper samples

## Disposition

This read-only collection is bound to source commit
`3829a7b2dccb2c651c85a920e19c2f705607ab6d` on
`codex/samples-bankr-doppler`, Robinhood Chain ID `4663`, public RPC
`https://rpc.mainnet.chain.robinhood.com`, and the canonical Airlock
`Create(address,address,address,address)` event emitted by
`0xeb7c034704ef8dcd2d32324c1545f62fb4ad0862`.

The three disjoint inclusive windows total 629,227 L2 blocks, below the
750,000-block cap:

| Window | Inclusive range | Blocks | Canonical Airlock events |
|---|---:|---:|---:|
| historical V1 | 10,976,000–10,978,000 | 2,001 | 1 |
| generational V2–V5 | 11,623,000–12,200,000 | 577,001 | 842 |
| recent head | 12,409,000–12,459,224 | 50,225 | 96 |

There was no wallet filter. The collector queried Blockscout and the public RPC
sequentially with concurrency 1. The successful final run observed RPC head
`12,465,772` at `2026-07-17T22:07:59Z`.

The current-window result is:

| Reviewed profile / envelope / orientation | Count | Classification |
|---|---:|---|
| V3 / direct Airlock / token > WETH | 4 | active, reviewed |
| V3 / ERC-7579 / token > WETH | 40 | active, reviewed |
| V3 / ERC-7579 / token < WETH | 1 | active, reviewed |
| V4 / direct Airlock / token > WETH | 3 | active, reviewed |
| V4 / ERC-7579 / token > WETH | 28 | active, reviewed |
| V1, V2, V5 | 0 | historically observed; not observed in this recent slice |
| non-reviewed Airlock shapes/targets | 20 | fail closed; not counted as Bankr detector misses |

V1–V5 are each represented by exact successful historical RPC samples. The
sample set has 11 transactions and covers both reviewed execution envelopes,
both token orientations where observed, a current direct V3, and the single
current reverse-orientation V3. All 11 samples passed canonical block hash,
receipt status, exact curve-profile pattern, exact envelope, canonical Airlock
event identity, pool identity, and two-position receipt checks. The ERC-7579
samples additionally contain the pinned `handleOps` / account selector / zero
mode / Airlock target / create selector shape and one successful
`UserOperationEvent`.

Direct-Airlock V5 remains **unsupported**, not merely inactive: the reviewed V5
profile requires the pinned ERC-7579 envelope. Unknown curve/account shapes
remain unsupported and fail closed. No result loosens pins or introduces global
selector dispatch.

## Paper plans and limitations

The machine artifact embeds six existing deterministic paper quote records:
V2 ERC-7579, V4 direct and ERC-7579, reverse V4 ERC-7579, V5 ERC-7579, and
reverse V5 ERC-7579. Each includes the fixed tiny WETH entry, 100 bps entry
minimum, complete full-position exit and exit minimum, simulated round-trip
return, source-fixture SHA-256, `execution_eligible=false`, and
`broadcast=false`.

The recent active V3 direct and reverse samples are new receipt/RPC evidence in
this task. The repository does not export standalone V1 or V3 quote JSON
fixtures at this source commit, so this branch does not invent or duplicate
quote math for them. Existing Bankr quote tests did reconstruct V1 and V3 entry
and full exit successfully, and the V3 campaign aggregate in the read-only
research corpus reports generated entry/exit plans. Integration should use the
shared quote implementation to export exact per-transaction V3 plans if a
standalone JSON fixture is required. This is the main unresolved artifact gap.

The 939 canonical Airlock events are an inventory, not 939 full replays. Eleven
were fully sampled; 930 remain unreplayed. The 20 current non-reviewed shapes
are reported separately rather than mislabeled as quote mismatches or detector
misses. Targeted samples had zero scan misses, zero classification/identity
mismatches, and the six imported deterministic quote fixtures had zero replay
test mismatches.

## Integrity

- `collect_bankr_doppler.py` SHA-256:
  `57714d1dbed9fd6b312a269c0ee14749099e15b5099b2d4371e23666a524356d`
- `evidence.json` SHA-256:
  `3b336ee0bf9b80fae74fe33e80b16cb5b9e7717caa620bb710d191ebec04747c`
- Each scan window in `evidence.json` binds its ordered Blockscout event
  inventory with a separate SHA-256.
- Each targeted sample binds normalized transaction, receipt, and canonical
  block RPC responses with separate SHA-256 values.
- Each embedded paper plan binds the exact read-only source fixture with
  SHA-256.

## Commands and results

```text
git switch -c codex/samples-bankr-doppler
# created from exact 3829a7b2dccb2c651c85a920e19c2f705607ab6d

python3 -m py_compile \
  hermes-feed/research/launchpads/samples/bankr_doppler/collect_bankr_doppler.py
# exit 0

python3 \
  hermes-feed/research/launchpads/samples/bankr_doppler/collect_bankr_doppler.py
# exit 0; 629,227 blocks, 939 canonical events, 96 current classifications,
# 11 exact targeted samples, 0 sample misses, 0 sample mismatches

cargo test -p hermes-feed --lib bankr_receipt_quote::tests:: -- --test-threads=1
# 43 passed; 0 failed; 360 filtered out

cargo test -p hermes-feed --bin hermes-launchpad-reconcile bankr_v -- --test-threads=1
# 4 passed; 0 failed; 23 filtered out

jq empty hermes-feed/research/launchpads/samples/bankr_doppler/evidence.json
# exit 0

shasum -a 256 \
  hermes-feed/research/launchpads/samples/bankr_doppler/collect_bankr_doppler.py \
  hermes-feed/research/launchpads/samples/bankr_doppler/evidence.json
# hashes recorded above
```

One intermediate collector rerun received HTTP 429 from the public RPC. No
partial artifact was published. The collector now uses bounded exponential
backoff for HTTP 429/5xx and still fails closed after six attempts.

## Safety and integration

No wallet, key, keystore, signer, transaction construction, broadcast, canary,
deployment, SSH, Droplet, server, or production mutation occurred. The only
writes are within
`hermes-feed/research/launchpads/samples/bankr_doppler/**`.

Integration should cherry-pick this branch commit, keep the machine JSON and
Markdown together, verify the two top-level SHA-256 values, and treat the
result as paper evidence only. Do not use it to broaden V5 to direct Airlock,
admit any of the 20 non-reviewed current shapes, alter selector dispatch, or
enable execution. If exact standalone plans for the newly sampled current V3
transactions are required, the integration owner should export them through
the existing shared quote path without changing this evidence branch.
