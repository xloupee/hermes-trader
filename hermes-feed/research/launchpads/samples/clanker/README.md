# Clanker launchpad-wide paper samples — 2026-07-17

## Disposition

The bounded public-RPC scan met the missing-profile target. It produced **10 confirmed `extensionless_single_position` samples** and, separately, **10 confirmed `pinned_extension_five_position` samples**. All 20 confirmed records contain the strict receipt-end entry quote, 100 bps slippage floor, immediate full-position exit quote, and `0x351` simulated round-trip return bps. Every quote remains `execution_eligible=false` and `broadcast=false`.

This is paper evidence only. It does not authorize a canary, signing, broadcast, deployment, promotion, or any wallet action.

## Boundaries and provenance

| Field | Value |
|---|---|
| Source SHA | `3829a7b2dccb2c651c85a920e19c2f705607ab6d` |
| Branch | `codex/samples-clanker` |
| Chain / RPC | `4663` / `https://rpc.mainnet.chain.robinhood.com` |
| Scan range | `(12010617, 12460617]`, exactly `450000` L2 blocks |
| Start anchor | `12010617` / `0x214159a9f94e8c113de186cd63f71ea0f2b4fa6b1e2ee3e7eba6dc5b9ef71e82` |
| Cutoff anchor | `12460617` / `0x62f973b0212ab7580baa4576633a3cfdd3f0c51ddd33549622f02af98c02fc28` |
| Requested cap | `750000` L2 blocks |
| Stop reason | 10 strict extensionless confirmations reached |
| RPC concurrency | `1` |
| Factory / topic | `0xd3f2cc1731b7fd17f28798835c2e02f0a1839a94` / `0x9299d1d1a88d8e1abdc591ae7a167a6bc63a8f17d695804e9091ee33aa89fb67` |
| Expected-pins SHA-256 / Keccak-256 | `f1fa4f3080fc3d3c193a5de4424410bc747da784d465f118b6a67d2e74a93095` / `0x76b72032db60f30c777cba1c30c1939381b0af947257cfbcb07ef11caee76633` |
| Startup-snapshot SHA-256 / Keccak-256 | `dcd698e71ff30fbdcdec6ba7955a44826326eaf7e94a5014967699b6553e21ea` / `0x7f09404c8bef819103767f70aee58f6dff37ca7d2aa4c1e387552e3bbb1f3bb4` |
| Observer / reconciler Keccak-256 | `0xea548c61db549bbdd81671c8e60d0d84926714d1403bae94e2b4259ce8268cc3` / `0xbde70e2d15fad8662c491de18831bb88131de88d40147fd79bca950582a9ea60` |

The startup snapshot and an end-of-run fixed-cutoff `cast codehash` check matched all seven reviewed Clanker runtime commitments: factory, deployer library, PoolManager, static hook, locker, descending-MEV module, and pinned extension.

## Scan and reconciliation counts

| Counter | Result |
|---|---:|
| Exact factory/topic events | 1573 |
| `extensionsSupply == 0` event candidates | 13 |
| Nonzero-supply event candidates | 1560 |
| Extensionless candidates reconciled | 13 |
| Confirmed extensionless quotes | 10 |
| Nonzero-value extensionless candidates blocked fail-closed | 3 |
| Pinned-extension candidates reconciled | 10 |
| Confirmed pinned-extension quotes | 10 |
| Pinned-extension events retained but not receipt-reconciled | 1550 |
| Receipt failures / protocol-event misses | 0 / 0 |
| Identity / profile-shape / quote mismatches | 0 / 0 / 0 |

`extensionsSupply` is only a selection hint. A sample is called confirmed only when the existing strict Rust quoter validates the complete canonical transaction, receipt, block envelope, exact TokenCreated identity, PoolKey/Initialize, ordered liquidity positions, hook and MEV configuration, profile shape, and quote construction.

## Confirmed `extensionless_single_position`

| L2 block | Transaction | Token | Positions | Round-trip bps |
|---:|---|---|---:|---:|
| `12019369` | `0x26800bbaacc0826d35a0700b0abaf1551fa5b6a0b361063cc34bc3d97839b1a4` | `0xd21f7ec4a471e6431e7ec0a4fe4bc3d09f9c3b07` | 1 | `0x351` |
| `12062521` | `0x482ba4281769908520db69cc209e8b7adad180df839aeb9ab8804d6e78fa49e1` | `0x5c3e24b7816b8a5e883d970cf971f6e3e118db07` | 1 | `0x351` |
| `12129021` | `0x2141a040e4e9141d54ee9e0a03c899b537271ba4da628d90d3167bbbd4089f2f` | `0xb028087d3eb06a0305166e5d09ac54877d741b07` | 1 | `0x351` |
| `12143551` | `0x606549c5e6611e38958e7829a94c3aa43c1a56cf595fdbe9dec798d1e3894bbb` | `0x051b425a2c8ec3e556b39b8214bde9ae19839b07` | 1 | `0x351` |
| `12147649` | `0x34faa79b59ce918e19113c90919a4c4e8c39e71cae8f3bf1cc4419fe0c6c8e18` | `0xb57bea3257e0ff94402e6bccde421de311632b07` | 1 | `0x351` |
| `12169176` | `0xd1eceadc0bb21953f22335aec2713db58f29907c2743bab7bbed7b3c9613e6e7` | `0xe224b49a3338b174272983c6e976c87e45b11b07` | 1 | `0x351` |
| `12297678` | `0x0297db2b2c3f8f53edf1e1fdbf34cc6c8c2ccd44e2bf87010387c0f7478100ee` | `0xc85fb5d29497cd0282c714454bc7226c453ccb07` | 1 | `0x351` |
| `12303475` | `0xa3650537de9de33d2542a95d100811b66fc33f72725915a30244987ced1b8bb5` | `0xb3a82506ab7c00160f5d51a28a95cb1ed3bb6b07` | 1 | `0x351` |
| `12413636` | `0x9ac490027c5994e819deaf2b08512cc1b8cc6e3fe1f4417c6af4162161817ab9` | `0x8e9b42fda9c6e0e91835ae86299f576fc99efb07` | 1 | `0x351` |
| `12427504` | `0xff7dc5ca6cddddf18a81ef605d6a0195a94cd7159f23288ee4e7c74aa9cfa484` | `0x4ae3d5945876b66f3d01b6c46e8616ca85b63b07` | 1 | `0x351` |

Three additional exact zero-supply events were not admitted. Transactions `0xe3b7...1791`, `0x4fd6...e592`, and `0x67cf...3661` called the exact factory and selector but carried nonzero transaction value. The strict zero-initial-buy envelope rejected all three, so they are recorded as misses/blockers rather than extensionless confirmations.

## Confirmed `pinned_extension_five_position`

| L2 block | Transaction | Token | Positions | Round-trip bps |
|---:|---|---|---:|---:|
| `12385112` | `0x6c3ca9b289dab3b2e1fd9c5caa13bf55b6834931e835e984ff922e8f095161e6` | `0xafe234a06ad8c87386efe41123a5149d69e1cb07` | 5 | `0x351` |
| `12400911` | `0xb457330c9926bf9802bf3fb3b150cdd2ae1080f776651592cd4526f2af584c2b` | `0xa17817f243c977aefbad96e20e96c24c214dbb07` | 5 | `0x351` |
| `12403078` | `0xb5594ff9ef68b868752edc98f068eb34e4073123635bbd8fe14af429eabee86e` | `0xbaf4a2393e1ebcb3a2a6237ff5abb213ccc68b07` | 5 | `0x351` |
| `12412701` | `0xc097fe12958b8e3806013cf489c9a0bceba300656672cfbe8ec1798c43ddfb4b` | `0x712e39e20bf5773dbfe8b7a5dafecc86c9800b07` | 5 | `0x351` |
| `12421339` | `0x3c52a620becea42934dc1279bbfb2aa188c5d9004c7e811bb2c37f38ec820e26` | `0x1de91395e32753b2f591074c74229f1758e34b07` | 5 | `0x351` |
| `12421930` | `0x172af67f5c00a1d434a5fc1b77d317dfc4c4d21091584e4b7686307e7a7655a9` | `0x0e6a8675198473a07690ad088f385943fc0e9b07` | 5 | `0x351` |
| `12422602` | `0xb9c1849cbd8e797596b8e21e0c94b1263784e2f7105f92c4a16df6e8f7ed5256` | `0xef7c320238aa3a3c7daee5623f0f5e2849af8b07` | 5 | `0x351` |
| `12423178` | `0x3302f0e3c2c0250287f45eb2d1f19ddf8e1899b759d4182f7940e7663b100033` | `0xb770d701b7d61d82cdbc212153ca98b7cee7ab07` | 5 | `0x351` |
| `12424038` | `0x920b3292b192a5d05cca82f1c2870a90ab4469c02af816ed54734e2907b50954` | `0x842ce4755425835f4d44b6b404366a8c50471b07` | 5 | `0x351` |
| `12444591` | `0x55c0224a909455e57918a4f07531870910cc2c5e4e7dfe2afc20c8bafcffba5b` | `0x9c5f5d869d2ace9f4106ac67dd3a20679eb52b07` | 5 | `0x351` |

The 10 pinned-extension rows are a bounded sample, not a claim that all 1560 nonzero-supply events were receipt-quote eligible. All 1560 exact event records remain retained and distinguishable as nonzero-supply candidates in the raw inventory; 1550 were intentionally not receipt-reconciled after the requested extensionless target was satisfied.

## Files and integrity

- `clanker-paper-samples-2026-07-17.json` is the authoritative consolidated machine report. It includes complete quotes, candidate dispositions, counters, pins, hashes, and assumptions.
- `observed-startup-snapshot.json` is the exact startup snapshot consumed by the observer and reconciler.
- `raw/events-*.json` contains all 1573 exact factory/topic log records in nine non-overlapping 50000-block chunks. The machine report binds each file by SHA-256 and exact bounds.
- `raw/observer-*.jsonl` and `raw/reconciliation-*.jsonl` preserve the exact two concurrency-1 reconciler inputs and outputs.

## Reproduction and checks

Key collection/reconciliation commands were:

```sh
cast logs --rpc-url https://rpc.mainnet.chain.robinhood.com \
  --from-block FROM --to-block TO \
  --address 0xd3f2cc1731b7fd17f28798835c2e02f0a1839a94 \
  0x9299d1d1a88d8e1abdc591ae7a167a6bc63a8f17d695804e9091ee33aa89fb67 --json

./target/release/hermes-launchpad-reconcile \
  --acquisition replay --input OBSERVER_JSONL \
  --expected-pins config/launchpad-expected-pins.production.json \
  --observed-startup-snapshot research/launchpads/samples/clanker/observed-startup-snapshot.json \
  --rpc-url https://rpc.mainnet.chain.robinhood.com --concurrency 1 \
  --paper-amount-in-wei 1000000000000000 \
  --paper-max-amount-in-wei 10000000000000000 --paper-slippage-bps 100
```

Static checks assert exact 450000-block coverage, non-overlapping chunks, 1573 event rows, 13 zero-supply hints, 10 strict extensionless confirmations, 10 strict pinned-extension confirmations, unique transactions, profile position counts `1` and `5`, zero identity/profile/quote mismatches, and hard-disabled execution/broadcast flags.

Recorded results:

- Consolidated `jq -e` invariants: `true`.
- Raw-inventory `jq -s -e`: `1573` rows and `1573` unique transaction/log-index pairs, `true`.
- `cargo test clanker_receipt_quote --lib`: `10 passed; 0 failed; 393 filtered out`.
- `cargo test --bin hermes-launchpad-paper clanker`: `3 passed; 0 failed; 27 filtered out`.
- Start/cutoff anchor refetch: both hashes unchanged.
- Cutoff Clanker runtime pins: `7/7` matched, `0` mismatched.

## Assumptions, risks, and integration

- The scan is launchpad-wide and wallet-independent. No wallet filter was used.
- A nonzero extension supply was used only to select the separate pinned-extension sample. The strict quoter, not that hint, determined the final profile.
- This was not a live detector campaign, so live detector-miss rate is not measured.
- The existing reconciler has no standalone arbitrary historical-triplet CLI. Capabilities-only replay frames were used to reach the reviewed strict quote code without changing shared source. A full campaign finalizer expects transaction inventory fields that this focused historical input does not contain; quote/replay integrity is therefore checked with the targeted Clanker library tests and the preserved reconciler output.
- Integration should cherry-pick this commit or copy only `research/launchpads/samples/clanker/**`. No shared source, config, scripts, manifests, Cargo files, or other launchpad research should be replaced.
