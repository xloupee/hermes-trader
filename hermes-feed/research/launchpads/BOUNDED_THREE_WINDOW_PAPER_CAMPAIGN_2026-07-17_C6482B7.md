# Bounded three-window paper campaign (`c6482b7`)

## Verdict

Exactly three sequential five-minute public read-only windows were completed
from source commit `c6482b7000dd68bc1e23edc5f80a1525499ecf06`. All three are complete,
independent, accepted evidence; no partial or fourth window exists. The
aggregate report remains fail closed: no launchpad is paper-evidence ready,
canary-authorized, execution-eligible, or promotion-ready.

All activity was local and paper-only. No wallet, keystore, signer, broadcast
path, canary, deployment, Droplet, or server was accessed. The immutable
runtime root is
`hermes-feed/.runtime/bounded-three-window-c6482b7-20260717T184145Z`.
The companion machine record is
`BOUNDED_THREE_WINDOW_PAPER_CAMPAIGN_2026-07-17_C6482B7.json`.

## Collection and integrity

- Each probe duration was 300 seconds total, including about ten seconds of
  warmup. Ground-truth coverage begins at the start anchor after `connected`;
  the first non-warm/scored frame arrived about ten seconds later.
- The expected-pin file stayed fixed at SHA-256
  `f1fa4f3080fc3d3c193a5de4424410bc747da784d465f118b6a67d2e74a93095`
  and Keccak-256
  `0x76b72032db60f30c777cba1c30c1939381b0af947257cfbcb07ef11caee76633`.
- Every startup snapshot matched all 41 pins. Each used 83 logical RPC
  requests in 83 attempts with zero retries, rate limits, server errors, or
  transport errors.
- Each feed has exactly one `connected` and one `coverage_closed` record,
  with zero reconnects, sequence gaps, missing frames, or reordered frames.
- The correct private FIFO topology was used: probe `--record` output was
  teed to `raw-feed.jsonl` and observer stdin; probe stdout contains metrics.
- Reconciliation/finalization completed after EOF. Each manifest was
  published last and binds nine canonical artifacts.
- No FIFO, symlink, or `.partial` residue remains. The tuple directory and
  seven executables are mode `0500`; evidence directories are `0700`; files
  are `0600`.
- The readiness and report outputs were independently regenerated in A/B/C
  order and matched the published bytes exactly.

| Window | Snapshot L2 | Scored L2 range | L1 block / timestamp |
|---|---:|---:|---:|
| A | `12,344,112` | `12,344,191–12,347,164` | `25,554,283` / `1784313852` |
| B | `12,347,890` | `12,347,968–12,350,939` | `25,554,314` / `1784314231` |
| C | `12,351,487` | `12,351,559–12,354,552` | `25,554,344` / `1784314593` |

| Aggregate artifact | SHA-256 | Keccak-256 |
|---|---|---|
| Authoritative readiness | `5b625cc000d2123531ddf0b0e8ee3470cdb2d2e961a66582a19118c3ac22d7f9` | `0xce1f5b99f827e881bdf319331e46167f5be6be9b3013f33fe7e7424d7279a934` |
| Evidence report | `6b8e6b38020a8c4c54edca6246b68f073156256bc3d06eb65593918f7ac481a2` | `0x0abb875054d694a7f5ce441a7c65103e9f9fd7b759a485590c6fa64019a3da5a` |

## Launchpad results

Report latency is source receive (`frame_received_unix_ns`) to observer
classification latency on confirmed rows. It does not include the separate
independent receipt/event reconciliation RPC duration. Round-trip values are
simulated quote-asset return basis points.

| Launchpad | Confirmed / eligible | p50 / p95 / p99 | Entry / exit | FP / misses | Round trip p50 |
|---|---:|---:|---:|---:|---:|
| Bow | 0 / 0 | n/a | 0 / 0 | 0 / 1 | n/a |
| LaunchHood V3 | 6 / 6 | 0.366 / 0.552 / 0.552 ms | 6 / 6 | 0 / 0 | 9,801 bps |
| Clanker | 41 / 41 | 0.435 / 1.001 / 1.122 ms | 41 / 41 | 0 / 0 | 849 bps |
| Bankr/Doppler | 28 / 28 | 0.371 / 0.932 / 1.268 ms | 28 / 28 | 0 / 0 | 97 bps |
| Pons | 151 / 0 | 0.317 / 0.703 / 0.946 ms | 0 / 0 | 0 / 0 | n/a |
| Hood | 0 / 0 | n/a | 0 / 0 | 0 / 0 | n/a |
| Flap | 152 / 0 | 0.303 / 0.767 / 0.975 ms | 0 / 0 | 0 / 3 | n/a |

All identity, direction, prediction, and quote mismatch counts are zero. Every
eligible observation has an independent entry and full-position exit plan;
every plan has `broadcast=false` and remains execution-gated.

### Bankr/Doppler

All 28 eligible confirmations were `CurveTicksV3`: 27 used pinned ERC-7579
execution and one was direct Airlock. V1, V2, V4, and V5 each had zero
observations. This campaign therefore adds no reverse-V5 or V4 positive
evidence. The ERC-7579 and V3 profile minimums pass, but total eligible count
is only 28/100 and direct Airlock is only 1/10; readiness remains false.

### Bow

One Bow ground-truth event was correctly left unclaimed at transaction
`0x6460c0afc4cbdac9e5e5b62db5eb982a92d4affc7051ccf89daa1e5df332f100`.
It failed the strict V3 transaction/receipt launch envelope. That is a scored
detector miss, not a successful fail-closed classification, so Bow cannot be
promoted.

### Pons

All 151 authoritative confirmations were legacy-generation launches. None
was current-generation or quote eligible. The zero detector-miss count does
not supply current-generation evidence; the required profile remains 0/10.

### Hood and Stonks suppression

No Hood current-curve or migrated-V3-boundary sample occurred. There were no
structural Stonks launchpad or shared-Airlock suppression records. This is
absence of evidence, not positive suppression coverage.

### Flap

Flap remains discovery-only. The three ground-truth transactions below are
canonical `Portal.TokenCreated` events emitted by Portal
`0x26605f322f7ff986f381bb9a6e3f5dab0beaeb09`; their event-data source/creator
is the VaultPortal proxy `0xe9f7ab7de8fb8756acbb6a1cd13316a43308197b`:

- `0x14f3fdb081d9af36e9dd06115654392a22de09ce167186c19757576b830ee1c9`
- `0xcbb6e9d7d4bcd78b7124eafcf473b7a101a041b7415ee96eabb9f054ab3ecd8f`
- `0x162df0a7add4d2aa017b51fc2c18863502210c08ec608f959d1feab8a0dad202`

They had no observer claim because the VaultPortal selector/path remains
intentionally unadmitted. Their generic reconciliation blocker is
`flap_discovery_only_prediction_and_quotes_unavailable`, but that is not by
itself the cause of these misses: the 152 direct Portal claims also lack
quotes. The three remain scored detector misses, and Flap remains explicitly
blocked by `discovery_only_launchpad`.

## Readiness and next bounded work

The campaign satisfies the three-independent-window count but no launchpad
satisfies the complete promotion policy. The concrete evidence gaps are Bow
valid launch profiles, LaunchHood volume (6/100 and 6/10), Clanker total
volume plus extensionless coverage, Bankr total/direct/V1/V2/V4/V5 coverage,
current-generation Pons, both Hood profiles, and non-discovery Flap semantics.

Before any further live collection, the Bow miss should be classified and
covered by a fixture. Future paper-only campaigns should be targeted at the
missing envelopes rather than treated as generic promotion evidence. No
canary, wallet, broadcast, deployment, or server action is authorized.
