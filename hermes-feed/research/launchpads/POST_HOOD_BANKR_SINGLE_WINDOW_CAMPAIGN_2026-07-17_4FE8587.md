# Post-Hood/Bankr single-window paper campaign (`4fe8587`)

## Verdict

One fresh five-minute window was collected from exact source commit
`4fe85872a97c2571b76c07e351d6603cbc8473fb`. The originally planned second
and third windows were deliberately not started after the test scope was
reduced. The completed window is valid evidence, but it is not a meaningful
multi-window promotion sample. Every readiness, canary, execution, and
broadcast decision remains false.

All activity was local, read-only, and paper-only. No wallet, keystore,
signer, broadcast path, canary, deployment, Droplet, or server was used.

The immutable runtime root is:

`hermes-feed/.runtime/post-hood-bankr-4fe8587-20260717T162924Z`

The machine-readable summary is
`POST_HOOD_BANKR_SINGLE_WINDOW_CAMPAIGN_2026-07-17_4FE8587.json`.

## Collection and integrity

- Worktree branch: `codex/post-hood-bankr-campaign`.
- Exact source: `4fe85872a97c2571b76c07e351d6603cbc8473fb`.
- Expected-pin Keccak:
  `0x76b72032db60f30c777cba1c30c1939381b0af947257cfbcb07ef11caee76633`.
- Fresh startup snapshot: L2 block `12,264,534`, hash
  `0x7a258c84face664390ff729f19cc9382d27b85fce0a92b3172a188eb01367368`.
- All 41 production pins matched. Snapshot acquisition made 83 logical RPC
  requests in 83 attempts, with zero retries, rate limits, server errors, or
  transport errors.
- Scored range: L2 blocks `12,264,633–12,267,623` inclusive, 2,991 blocks.
- Feed continuity: one `connected`, one `coverage_closed`, zero reconnects,
  connect/read/disconnect errors, sequence gaps, missing frames, or reordered
  frames.
- The correct private mode-`0600` topology was used: `hermes-feed probe
  --record` to FIFO, `tee` to `raw-feed.jsonl`, and observer stdin. Probe
  stdout contains metrics only.
- Reconciliation and finalization ran after EOF. The completion manifest was
  published last and binds all nine canonical session artifacts.
- No FIFO, symlink, or `.partial` residue remains. The tuple directory and its
  seven executables are mode `0500`; campaign directories are `0700`; all
  evidence files are `0600`.
- Both local and campaign fixture suites passed before collection.

The single-window authoritative readiness and report were regenerated
byte-identically from the completed manifest:

| Artifact | SHA-256 | Keccak-256 |
|---|---|---|
| Authoritative readiness | `c8b6e6cefe49497aca7d7416f52eb4b4834827aa815aeced3269cd8f8e8b04db` | `0xfa5619d9bafeece4d5ebd05cf2be8c79bff478ed9b2609a9983b05b471443bd7` |
| Evidence report | `ac6ae13953edde485a6b538f9276ff63d93b540a74c726714ab1a1eb7e9ee50a` | `0xd2715ce02d76fbcd09104f98d4bd924a7a8c674ad02b3327012fcd2ca18c74c5` |

## Exact runtime tuple

| Executable | SHA-256 | Keccak-256 |
|---|---|---|
| `hermes-feed` | `c8513d61e3c2bb012d83e1250bedf62b9453b07e22cef5a99a537dd6136ad49e` | `0x5d461c9c2831f988ab487a919999d466154cdee434bfde47cd36ee4b6866d757` |
| `hermes-launchpad-pin-snapshot` | `865d9fb705c8d87f62920ff6e740f6cf000cbefa1f48728c2d9f68bac7c3230b` | `0x883a37bc8b22f8d80327c1b5560026f25374efb48442756a4a6ae6a9b6adc3d4` |
| `hermes-launchpad-evidence-report` | `2ef4e935c4e3ca52797a4cca332b864de0fff27ce1afc33496835fda84b35121` | `0x4ba97881d7d6bbc9ac15a1bfce8dfb7a53b9f2f20864a9dd052f939359e072b1` |
| `hermes-launchpad-paper` | `c12bdc724a5f68c45502545e3f0438eb11a12ddb483af7ae0c1b10f2029314ba` | `0x8677d213d213d3c421d32d21aae8c0638f7e625f96073284a149a821c460cea1` |
| `hermes-launchpad-reconcile` | `1c09695809589955758fce8e7381e42ef86bc4d537ebefd51f97473a4b188ea1` | `0x7168bcd89d47b6e1c73162674083f6bcdef9a6472dc8a0bc0dff80a3caf5acc8` |
| `hermes-launchpad-chain-head` | `4f34f65853d6b71afcb4305fa0f31c3944439521b774f7ec7753bc14ed43aee8` | `0x71a3d8a56d6e7198ff62f2ba7c219d84ef758190e2cb87caf0deb1a03f59c3a5` |
| `hermes-launchpad-readiness` | `52b46d5c0b585b92df22e18337eb3d8eb997937ab9c3c057bdd020011a35a4ef` | `0x967f147d91f3bedb1faaa3084cf020a053e1934f8018b437cabec44e3bcf22c9` |

## Launchpad results

Latency is observer-to-confirmed receipt/event reconciliation latency.
Round-trip values are simulated quote-asset return basis points.

| Launchpad | Confirmed / eligible | p50 / p95 / p99 | Entry / exit plans | FP / misses / identity / direction / prediction / quote | Round trip |
|---|---:|---:|---:|---:|---:|
| Bow | 0 / 0 | n/a | 0 / 0 | 0 / 0 / 0 / 0 / 0 / 0 | n/a |
| LaunchHood V3 | 3 / 3 | 0.302 / 0.318 / 0.318 ms | 3 / 3 | 0 / 0 / 0 / 0 / 0 / 0 | 9,801 bps |
| Clanker | 0 / 0 | n/a | 0 / 0 | 0 / 0 / 0 / 0 / 0 / 0 | n/a |
| Bankr/Doppler | 5 / 5 | 0.373 / 0.508 / 0.508 ms | 5 / 5 | 0 / 0 / 0 / 0 / 0 / 0 | 97 bps |
| Pons | 39 / 0 | 0.379 / 3.633 / 9.268 ms | 0 / 0 | 0 / 2 / 0 / 0 / 0 / 0 | n/a |
| Hood | 0 / 0 | n/a | 0 / 0 | 0 / 0 / 0 / 0 / 0 / 0 | n/a |

### Bankr/Doppler

All five authoritative confirmations were pinned ERC-7579 `CurveTicksV5`
launches with token greater than WETH, initialize tick `+229200`, fee `7000`,
spacing `200`, and receipt ranges `[119200,229200]` and
`[-887200,119200]`. Each produced independent `0.001 WETH` entry sizing,
1% slippage bounds, a full-position exit quote, and a simulated immediate
round-trip of 97 bps. All plans remain execution-gated and broadcast false.

No token-less-than-WETH reverse-V5 launch occurred, so this window does not
exercise the new reverse-V5 detector. It must not be reported as reverse-V5
positive evidence.

### Pons

The 39 confirmed observations were legacy generation and remain
discovery-only. Two current-generation factory launches were ground truth but
not observer claims and therefore remain detector misses:

- `0xd7b246383c97a8b511893996020a26b1b82de1c03eef191837428df62be1432b`
- `0x76450fe5a04c11ff97916a3fc7af2a6bae289124dc60b6c783d951893ecd5103`

Both failed closed with `pons_quote_error: transaction, receipt, or paper
policy envelope is invalid`. This window provides no current-generation Pons
prediction or quote positive.

### Hood and Stonks

No Hood launch, buy, sell, or migrated-V3 boundary sample occurred. The new
launch-only action taxonomy and migrated scoped quote were therefore not
exercised. No Stonks V3 or shared-Airlock suppression record occurred, so
suppression evidence is also absent rather than positive.

### Flap

Flap remained discovery-only. Thirty-five observer candidates were retained
outside the six-launchpad readiness report; none became a scored quote or
execution plan.

## Readiness

The report accepts one complete live window and excludes zero partial windows.
One window is below the fixed minimum of three, and every launchpad is below
the 100 eligible-observation threshold. Pons additionally has two detector
misses. No launchpad is eligible for promotion.

The bounded next evidence action, when longer testing is resumed, is another
same-tuple campaign specifically seeking reverse V5, current Pons, Hood
migrated/action-taxonomy, and Stonks positives. No canary or server work is
authorized by this evidence.
