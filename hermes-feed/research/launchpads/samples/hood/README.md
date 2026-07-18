# Hood bounded receipt samples

Disposition: **10 exact `current_curve` quotes collected; 5 of 10 discovered
`migrated_v3_boundary` scopes produced exact quotes and 5 failed closed.** This
is paper evidence only. It does not authorize signing, broadcast, deployment,
or promotion.

## Scope and provenance

- Source SHA: `3829a7b2dccb2c651c85a920e19c2f705607ab6d`
- Chain: Robinhood Chain `4663`
- Public read-only RPC: `https://rpc.mainnet.chain.robinhood.com`
- RPC/candidate concurrency: `1`
- Aggregate bounded scan: exactly `750,000` L2 blocks across two inclusive,
  disjoint ranges:
  - migration span `10,079,778..10,794,005` (`714,228` blocks), hashes
    `0x3078ae0a96e0ba2f7bf2486af344589ef2f21111f14aacd40fb178ddf02c5c7a`
    through
    `0x1e375ae5befed86bb9e96b20f77f0419ef427dad2b2c9848f9cee3500a42eab0`;
  - current-curve span `11,700,000..11,735,771` (`35,772` blocks), hashes
    `0xf4289e7270acbb066701b36aae0d3cfc1a6ae98872007f71a7719e0522b94ceb`
    through
    `0x5c2239cfea25ff0637d45a9ff79f6dbabcded062e7962d5c6503fa5f9fee9f43`.
- Every endpoint hash matched before and after collection.
- The scan used 16 continuous chunks no larger than 50,000 blocks. Its final
  pass made 41 logical/HTTP attempts with zero retries or rate limits.
- All 20 runtime checks (10 Hood identities at both range endpoints) matched
  exact expected byte lengths and keccak256 hashes. There is deliberately no
  universal pool runtime hash: pool identity is pair/fee/CREATE2/factory
  scoped.

The scan found 3,545 distinct canonical logs with zero duplicates: 2,929
`TokenCreated`, 586 `Trade`, 10 `Graduated`, 10 factory `Migrated`, and 10
migrator `V3Migrated`. The canonical normalized log stream is bound by SHA-256
`2db31fe7ad0fe19acadbbea49876268fbd93bb52d2f192ebd4ba5061c4df03c4`.
The compact machine artifact retains the chunk manifest, event counts, all
candidate identities, and this digest rather than committing 104 MB of
redundant raw log payloads.

## Current curve result

The selector preflight checked 19 event-derived candidates. Nine were wrappers
or did not have the exact direct selector for their event action. The remaining
10 all passed transaction, canonical receipt event, transfer, fixed-block
state, direction, entry, slippage, and full-exit reconciliation:

| Action | Exact quotes |
|---|---:|
| Launch (including any same-receipt initial buy) | 4 |
| Ordinary buy | 3 |
| Ordinary sell | 3 |

Each quote uses an independent fixed `0.001 ETH` input, a `0.01 ETH` hard cap,
and `100 bps` slippage. Every entry and immediate full-position exit is emitted
verbatim in `replay.json`. All 10 are `execution_eligible=false` and
`broadcast=false`.

## Migrated V3 boundary result

Exactly 10 `(transaction hash, token, pool, tokenId)` scopes were discovered
across five transactions. Transaction-level deduplication was not used:

| Transaction topology | Scopes | Exact quotes | Blocked |
|---|---:|---:|---:|
| `0xb44053ba...687504` batch | 5 | 3 | 2 |
| `0x6f2eda66...922ced` batch | 2 | 2 | 0 |
| Three single-scope receipts | 3 | 0 | 3 |
| **Total** | **10** | **5** | **5** |

For the five successful scopes, receipt verification preserved exact token,
pool, tokenId, liquidity, log ordering, reconstructed terminal boundary, and
fixed-block pool state before generating the existing migrated entry and
full-exit plans. The other five have no plan. Their common blocker is
`hood_migration_strict_receipt:Hood graduation or migration receipt topology is inconsistent`.
This is the concrete reason the requested migrated target reached 5 rather
than 10 available quotes; the verifier was not weakened to fill the target.

## Artifacts

- `scan.json`: bounded ranges, anchors, chunk counts, normalized-log digest,
  and launchpad-wide candidate identities.
- `pins.json`: exact runtime identity observations at both scan endpoints.
- `replay.json`: current and migrated dispositions, native receipt quote
  records, batch scope keys, blockers, and outcomes.
- `collect.mjs` and `check-pins.mjs`: deterministic serial scan and pin-check
  helpers. The receipt/state replay used the existing base-SHA
  `hermes-feed` APIs through an out-of-tree temporary Rust driver; no shared
  source or fixture was edited.

## Reproduction and checks

Commands used from the repository root:

```sh
git switch -c codex/samples-hood
node hermes-feed/research/launchpads/samples/hood/collect.mjs
cargo build --release --bin hermes-launchpad-pin-snapshot --bin hermes-launchpad-reconcile
cargo run --release \
  --manifest-path /private/tmp/hermes-hood-samples-replay-35ed/Cargo.toml \
  --target-dir 'hermes-feed/target' -- \
  hermes-feed/research/launchpads/samples/hood/scan.json \
  hermes-feed/research/launchpads/samples/hood/replay.json
node hermes-feed/research/launchpads/samples/hood/check-pins.mjs
jq -e . hermes-feed/research/launchpads/samples/hood/{scan,pins,replay}.json
git diff --check
```

The targeted replay result was current `10 available / 0 blocked`, migrated
`5 available / 5 blocked`, with `5` topology-verified migration scopes and no
runtime-pin mismatch. Static integrity additionally requires exact source SHA,
chain `4663`, aggregate scan size `750000`, matched before/after anchors,
10 discovered migration scopes, 10 current quotes, and no plan on a blocked
scope.

## Assumptions, risks, and integration

- The two disjoint ranges are intentional: together they consume the exact
  750,000-block cap while covering all 10 known migration scopes plus a later
  current-curve interval. They are not represented as one continuous window.
- Candidate selection is launchpad-wide and deterministic; no wallet filter was
  used. The 3,355 event-derived current candidates are a scan inventory, not a
  claim that all are direct or quote eligible.
- Historical state availability and RPC canonicality were accepted only after
  endpoint re-reads. A future re-run must create new artifacts rather than
  silently overwrite these bytes.
- The five strict migration failures remain an unresolved implementation or
  historical-topology coverage gap for the integration owner. They must remain
  blocked unless a separately reviewed verifier change explains each exact
  receipt; this samples branch must not change shared code.

Integration should import this directory only. Consume `replay.json` by
profile and scope key, count the five blocked migration scopes as explicit
misses, and never infer a migrated plan from `scan.json` alone. No other
repository path, lockfile, runtime, server, Droplet, wallet, key, signer,
transaction, or production state was changed.
