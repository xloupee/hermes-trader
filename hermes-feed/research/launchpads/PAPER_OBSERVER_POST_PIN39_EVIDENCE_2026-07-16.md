# Paper observer post-pin39 evidence: 2026-07-16

This report consolidates the current local paper-only evidence without claiming
readiness. No wallet, keystore, signer, broadcast path, Droplet, or server was
used. All quoted outcomes are receipt-end-state simulations with
`execution_eligible: false` and `broadcast: false`.

## Evidence boundary

The primary live artifact is
`.runtime/paper-session-20260716-pins39-fresh1`. It used the production
expected-pin authority `config/launchpad-expected-pins.production.json` and
the fresh 39-pin startup snapshot
`.runtime/launchpad-observed-startup-pins39-fresh-confirmed.json`:

- snapshot schema 4, chain 4663, L2 block `11714556`, block hash
  `0x22d13b3621ddfc361507dd3e680b3adddce1c57cc059eea13d173e61f468a158`;
- persisted start anchor: block `11715614`, hash
  `0x83f28ee24320bd10ab7869ae818be10ec0d6cf4fb932e446b79d80e30b3c9147`;
- persisted cutoff anchor: block `11718597`, hash
  `0xa58166d1ec7d9437f30acf7d4ac0aa552abe8b817db78cc05ba600d275f48542`;
- complete canonical scan of blocks `11715615..=11718597`: 2,983 blocks,
  48 primary event logs, 47 unique protocol keys, and two confirmations;
- 3,654 raw-feed rows, 3,655 final-code observer rows, 123 reconciliation
  rows, and 22 finalized rows; and
- every persisted artifact has mode 0600.

Collection used the intended local topology: `hermes-feed probe` wrote the
raw stream to a private FIFO, `tee` persisted `raw-feed.jsonl` while forwarding
the same bytes, and `hermes-launchpad-paper --input -` consumed stdin. Probe
stdout was isolated in `probe-metrics.jsonl`, not mixed into the raw stream.

The unified final-code replay is the authoritative scored form of this window:

- `.runtime/paper-session-20260716-pins39-fresh1/launchpad-paper-finalcode-replay.jsonl`
- `.runtime/paper-session-20260716-pins39-fresh1/reconciliation-evidence-finalcode-replay.jsonl`
- `.runtime/paper-session-20260716-pins39-fresh1/launchpad-paper-finalized-finalcode-replay.jsonl`

The replay includes independent V3 quote reconstruction, strict Pons legacy
eligibility, Hood `buyFor` detection and token prediction, and finalized exit
plans. Its backlog-scale latency is invalid and excluded. The table below
retains latency only from the original live observer records. Builds may have
overlapped fresh1 collection, so those percentiles remain **potentially
build-contaminated diagnostics**, not promotion-grade warm-state evidence.

## Fresh1 live score

A dash means there was no confirmed observation. `Missing identity` counts
independently required token, address-pool, or V4 pool-ID fields that were
absent; it is not silently converted into a mismatch.

| Launchpad | Truth | Confirmed | False positives | Missed (detector / coverage) | Latency p50 / p95 / p99 | Identity | Action prediction | Direction | Quote replay |
|---|---:|---:|---:|---:|---|---|---|---|---|
| Bow | 0 | 0 | 0 | 0 (0 / 0) | - | no activity | no activity | no activity | no activity |
| LaunchHood V3 | 1 | 1 | 0 | 0 (0 / 0) | 6.260 / 6.260 / 6.260 ms | token 1/1, pool 1/1; 0 missing/mismatch | 1/1; 0 mismatch | entry 1/1, exit 1/1; 0 mismatch | 1/1; 0 mismatch |
| Clanker | 0 | 0 | 0 | 0 (0 / 0) | - | no activity | no activity | no activity | no activity |
| Bankr/Doppler | 7 | 7 | 0 | 0 (0 / 0) | 6.126 / 13.914 / 13.914 ms | token 7/7, pool ID 7/7; 0 missing/mismatch | 7/7; 0 mismatch | entry 7/7, exit 7/7; 0 mismatch | 7/7; 0 mismatch |
| Pons | 37 | 37 | 0 | 0 (0 / 0) | 4.441 / 8.090 / 12.968 ms | 37 exact legacy/discovery-only; 0 identity eligible, 0 mismatch | 37/37; 0 mismatch | not eligible | not applicable 37 |
| Hood | 2 | 2 | 0 | 0 (0 / 0) | 7.374 / 7.374 / 7.374 ms on the one originally detected row | token 2/2; 0 missing/mismatch | 2/2; 0 mismatch | entry 2/2, exit 2/2; 0 mismatch | 2/2; 0 mismatch |

Bankr emitted 16 observer claims: seven confirmations, seven reverted
attempts, and two out-of-scope observations. Pons emitted 40 claims: 37
confirmations and three out-of-scope observations. Those reverted and
out-of-range records are not false positives. Flap emitted 52 discovery-only
claims and remains outside the scored six-launchpad set.

The absence of mismatch values must not be interpreted as readiness. The
persisted pre-fix fresh1 readiness row counted 37 intentionally unresolved
legacy Pons tokens and pools as 74 identity failures. The eligibility audit in
commit `cccd588` proved all 40 Pons rows (37 in range, three out of scope) are
exact legacy/discovery-only records and corrected re-finalization to zero
identity-eligible rows and zero identity mismatches. It still has
`current_generation: 0` and zero quote-eligible confirmations, so this changes
no readiness decision. Final-code Hood replay closes the prior selector miss
and token-prediction gap at 2/2. Bow and Clanker had no fresh1 truth at all.

## Fresh1 paper sizing, slippage, and exits

Every finalized plan independently applies a fixed `0.001 WETH` entry, 1%
entry slippage, a full-position immediate exit, and 1% exit slippage. Leader
amounts are not reused.

| Launchpad | Plans | Expected entry output | Entry minimum | Expected full exit | Exit minimum | Simulated round-trip return |
|---|---:|---:|---:|---:|---:|---:|
| LaunchHood V3 | 1 | `729729.244852920426979991` tokens | `722431.952404391222710191` | `0.000980107152180750 WETH` | `0.000970306080658942 WETH` | 9,801 bps |
| Bankr/Doppler | 7 | `904475.731970933213275501` or `904475.731970933213278599` tokens | `895430.974651223881142745` or `895430.974651223881145813` | `0.000009736782115644 WETH` | `0.000009639414294487 WETH` | 97 bps |
| Hood | 2 | `388879.762303340027827257` or `372973.604689439708957393` tokens | `384990.964680306627548984` or `369243.868642545311867819` | `0.000980100000000000 WETH` | `0.000970299000000000 WETH` | 9,801 bps |

Bow, Clanker, and Pons produced no finalized plan in fresh1. Bankr's 97 bps
immediate round trip is evidence against promotion even though all seven
independent quote replays matched.

## Quiet1 replay evidence and latency exclusion

Quiet1 is anchored to blocks `11682302..=11685271`, with start block
`11682301` hash
`0xe59df3c967edadfbb70366a60b9bc215a0ed95e76fb4446534f1d1555a89bffd`
and cutoff block `11685271` hash
`0xf0748ec34612921f2d68efb95a4eadc0b042cb9ddbdf150c01cbbc1d110ec607`.

**All quiet1 replay latency is excluded.** Replaying persisted frames measures
replay wall time and backlog, not live source-receive-to-observation latency.

The unified final-code reconciliation is in
`.runtime/paper-session-20260716-quiet1-finalcode-replay`:

| Launchpad | Truth | Confirmed | False positives | Missed | Out of scope / reverted | Independently matched quotes | Identity limitation in this artifact |
|---|---:|---:|---:|---:|---:|---:|---|
| Bow | 0 | 0 | 0 | 0 | 0 / 0 | 0 | no activity |
| LaunchHood V3 | 3 | 3 | 0 | 0 | 0 / 0 | 3/3 | token 3/3 and pool 3/3 matched |
| Clanker | 3 | 3 | 0 | 0 | 1 / 0 | 3/3 | token 3/3; no required identity mismatch |
| Bankr/Doppler | 2 | 2 | 0 | 0 | 4 / 0 | 2/2 | token 2/2; no required identity mismatch |
| Pons | 28 | 28 | 0 | 0 | 12 / 0 | 1/1 current-generation quote; 27 legacy not applicable | current row has one missing token and one missing pool; readiness identity mismatches 2 |
| Hood | 0 | 0 | 0 | 0 | 0 / 0 | 0 | no activity |

All eligible action, entry-direction, exit-direction, and independent quote
checks matched. Its nine finalized plans consisted of three
LaunchHood, three Clanker, two Bankr, and one current-generation Pons plan.
The fixed tiny-policy outcomes were:

| Launchpad | Expected entry output | Entry minimum | Expected full exit | Exit minimum | Return |
|---|---:|---:|---:|---:|---:|
| LaunchHood V3 | `729729.244852920426979991` | `722431.952404391222710191` | `0.000980107152180750 WETH` | `0.000970306080658942 WETH` | 9,801 bps |
| Clanker | `2977989.243985511988125632` | `2948209.351545656868244375` | `0.000084919129125191 WETH` | `0.000084069937833939 WETH` | 849 bps |
| Bankr/Doppler | same two entry variants as fresh1 | same two minima as fresh1 | `0.000009736782115644 WETH` | `0.000009639414294487 WETH` | 97 bps |
| Pons current generation | `663271.426017223774908828` | `656638.711757051537159739` | `0.000980106818839881 WETH` | `0.000970305750651482 WETH` | 9,801 bps |

This replaces the older fixed2 and partial-identity artifacts as the quiet1
score. It proves one unified final-code pass for LaunchHood, Clanker, Bankr,
and Pons. It does not cure Pons current-generation identity: the one eligible
row remains fail-closed because both token and pool predictions are absent.

## Clean1 release-build live evidence

The build-idle release window
`.runtime/paper-session-20260716-finalcode-clean1` used the correct private-FIFO
topology, stayed continuously connected, and anchored blocks
`11737706..=11740690` (2,985 blocks):

- start block `11737705`, hash
  `0xb0e97810a729bf289f45e5fda30ecbb9d582c806d6fc76cf3b2df4392ea54dc5`;
- cutoff block `11740690`, hash
  `0x4e3eae93b4ea48413498f45cca591633c9e35ffb1994c0a9adabca7b83bebd7b`;
- 3,657 raw rows and 3,658 observer rows; and
- only `connected` and `coverage_closed` probe states, with no disconnect or
  read error.

The live binary rejected one successful Bankr launch because a proven
45-byte token name canonically expanded `tokenFactoryData` from 928 to 960
bytes. Commit `583611d` narrows admission to the two proven canonical ABI
lengths and names of at most 64 bytes, while a real transaction fixture rejects
65 bytes and noncanonical padding. The exact raw bytes and anchored receipts
were then rescored in
`.runtime/paper-session-20260716-finalcode-clean1-post-long-name-replay`.
Replay latency is excluded; the latency column below is exclusively the
original build-idle live timing.

| Launchpad | Truth | Live confirmed | Post-fix confirmed | False positives | Post-fix missed | Live latency p50 / p95 / p99 | Quote result |
|---|---:|---:|---:|---:|---:|---|---|
| Bow | 0 | 0 | 0 | 0 | 0 | - | no activity |
| LaunchHood V3 | 1 | 1 | 1 | 0 | 0 | 0.840 / 0.840 / 0.840 ms | 1/1 matched |
| Clanker | 2 | 2 | 2 | 0 | 0 | 0.339 / 0.599 / 0.599 ms | 1/2 matched; one strict receipt-envelope quote blocked |
| Bankr/Doppler | 4 | 3 | 4 | 0 | 0 | 0.534 / 1.806 / 1.806 ms for the three live confirmations; rejected row 3.274 ms | 4/4 matched after the bounded fix |
| Pons | 24 | 24 | 24 | 0 | 0 | 0.354 / 2.406 / 3.360 ms | 24 legacy/not applicable |
| Hood | 0 | 0 | 0 | 0 | 0 | - | no activity |

The blocked Clanker row is not a known-profile quote failure. Transaction
`0xf0961eb5b33df616d5abccd9ee61ee0a6eb28a6a3d1738b0a04c326750784f01`
is a separate payable Tator-extension envelope: value `0.00809079 ETH`, an
unpinned extension at `0xa27b1986e5c7e5371cb6507f87918fbd0302ff5a`, two liquidity positions,
and an embedded pool swap. It is neither the supported extensionless
single-position profile nor the pinned-extension five-position profile and
must remain quote-blocked until its runtime, source semantics, layout, and
terminal-state replay have independent multi-proof review.

The post-fix replay has zero detector, coverage, identity, action, direction,
prediction, or independent-quote mismatches. It emits six paper plans:
LaunchHood one at 9,801 bps simulated immediate round trip, Clanker one at
849 bps, and Bankr four at 97 bps. Every plan contains the full-position
take-profit/stop-loss/max-hold exit policy and keeps both
`execution_eligible: false` and `broadcast: false`.

Clean1 is not claimed as a post-fix live zero-error window: its live binary
predated `583611d`, and one Clanker truth row remains quote-blocked pending a
separate envelope diagnosis. Its live timing is nevertheless the first
release-build, build-idle latency sample.

## Clean2 release-build live evidence and active-boundary replay

The second build-idle release window,
`.runtime/paper-session-20260716-post43d1caf-clean2`, ran for 600 seconds with
the private-FIFO topology and only `connected` and `coverage_closed` probe
states. Its fresh startup authority was
`.runtime/launchpad-observed-startup-pins39-post43d1caf.json`:

- 39 expected production pins validated at confirmed L2 block `11755893`, hash
  `0x8c0172eee16a369df7649519cb524eea53ad70506583e33e9782d38783d75286`,
  L1 block `25549373`, timestamp `1784254818`;
- expected-pin validation passed, including the independently required Bankr
  EIP-7702 designator and delegated Kernel runtime; observed hashes were not
  promoted into expected authority; and
- snapshot mode 0600 and SHA-256
  `d10a0c9d2aae451c63badb2a111dc5c46c9de2c6dfdec3c5f4ac0b4f80407dc1`.

The live window persisted start block `11756131`, hash
`0xd597bf886999d6e394a5d6e164c6e05cd2468b2345ef6ef614dbbec302a8ebc3`,
and cutoff block `11762102`, hash
`0x0116823651713b2fc65328a64b7533225c1956de52ad56dece639ce269b66bd6`.
The complete scored interval was `11756132..=11762102`, or 5,971 blocks. It
contains 6,458 raw rows, 6,459 observer rows, 150 reconciliation rows, and 30
finalized rows. The directory is mode 0700 and every file is mode 0600. The
live digests are:

- raw feed: `dcb4b021636e38b11047e561d2a114aaf4b3716720116ddda32867a8b51461ed`;
- observer output: `b30a3c241329e12e930d95877552f0bf9984e78285cc02e74240bfa418c9644a`;
- reconciliation: `0ba5f898edc17f5e232079acd35fc4f2864c7798fd7365bce215b8747ba009dd`;
  and
- finalized output: `f8f50a323b012c79b3e95cb7f4255ec14bd6a32c45ff9736e7f90c7003946efb`.

The latency column below is exclusively the original live release-build
timing. It is not replaced by replay timing.

| Launchpad | Truth | Live confirmed | Post-fix confirmed | False positives | Post-fix missed | Live latency p50 / p95 / p99 | Post-fix quote result |
|---|---:|---:|---:|---:|---:|---|---|
| Bow | 0 | 0 | 0 | 0 | 0 | - | no activity |
| LaunchHood V3 | 2 | 2 | 2 | 0 | 0 | 0.292 / 0.561 / 0.561 ms | 2/2 matched |
| Clanker | 12 | 12 | 12 | 0 | 0 | 0.471 / 2.426 / 2.426 ms | 12/12 matched after the bounded active-boundary fix |
| Bankr/Doppler | 5 | 5 | 5 | 0 | 0 | 0.359 / 0.490 / 0.490 ms | 5/5 matched |
| Pons | 59 | 59 | 59 | 0 | 0 | 0.307 / 2.699 / 3.845 ms | 59 legacy/not applicable |
| Hood | 0 | 0 | 0 | 0 | 0 | - | no activity |

Bankr emitted eight observer claims: five confirmations, two reverted
attempts, and one out-of-scope observation. Pons emitted 61 claims: 59
confirmations and two out-of-scope observations. None of those reverted or
out-of-range records is a false positive. Every eligible identity, action,
entry-direction, exit-direction, and independent-quote check matched.

The live `43d1caf` binary quoted 11 of 12 Clanker confirmations. The remaining
transaction,
`0xdecb471e034489fb24c4dfa4f4aa71d0af49b0e90835018b093011bfaa91d712`,
is a supported `pinned_extension_five_position` launch, not the unsupported
Tator envelope. Its token is pool `token0`, its initialize tick equals the
first position's lower bound (`-230400`), and that position therefore provides
active receipt-end liquidity even without a Swap event. The old global
nonzero-liquidity assertion incorrectly blocked the otherwise canonical
receipt.

Commit `bbef8b4` replaces that assertion with ordered receipt-derived active
liquidity, independently recomputes it during replay, and binds the exact live
transaction plus tamper negatives. The exact raw feed was rescored at
`.runtime/paper-session-20260716-post43d1caf-clean2-post-active-replay`: 6,459
observer rows, 151 reconciliation rows, and 31 finalized rows. Clanker is now
12/12 quote-available with zero detector, coverage, identity, action,
direction, prediction, or quote mismatch. The replay emits 19 independent
paper plans: two LaunchHood, 12 Clanker, and five Bankr. Their simulated
immediate round-trip returns remain 9,801, 849, and 97 bps respectively; all
plans contain the full-position exit policy and retain
`execution_eligible: false` and `broadcast: false`.

**All post-fix replay latency is excluded.** It measures persisted-frame
backlog, not live receive-to-observation time. Clean2 therefore proves exact
replay correctness for `bbef8b4`, but it is not a live zero-error window on
that commit: collection ran on `43d1caf`. A new release-build, build-idle live
window on exact `bbef8b4` or later remains required. The separate clean1
payable Tator-extension/two-position envelope remains unsupported and strictly
quote-blocked.

## Clean4 current-code live cohort and corrected Pons classification

The attempted clean3 window is excluded. Its `tee` persistence path failed
closed on `ENOSPC`, so it is not treated as a complete feed, reconciliation
window, latency sample, or readiness input. The failed clean3 directory and
snapshot were left untouched for forensic provenance.

Clean4 is the first complete live-only cohort for the current observer code.
The release-build, build-idle window at
`.runtime/paper-session-20260716-d00adac-clean4` used the private FIFO through
`tee` topology and only emitted the `connected` and `coverage_closed` probe
states. Its independently generated 39-pin startup snapshot was
`.runtime/launchpad-observed-startup-pins39-d00adac-clean4.json`:

- confirmed L2 block `11776795`, hash
  `0x7d36b7564740627818e179445a8b4a48339a3339199953ef1dcc749879379de0`,
  L1 block `25549548`, timestamp `1784256912`;
- all 39 production expected pins passed validation without promoting observed
  hashes into expected authority; and
- snapshot SHA-256
  `093180b4a1f339ddd65cb1b27af1c059e7d2897cb2533edc38f1a18695967592`.

The persisted start anchor is block `11776968`, hash
`0x3f5dd2f9d08061986968449a7ab39da9aa0303ac4d41555b46f462369af018ca`.
The cutoff anchor is block `11782950`, hash
`0x2f46f09656c28e7059be75b1b8a4dfdb05494fdd73d74f7f952767aa56862c5c`.
The complete scored interval is `11776969..=11782950`, or 5,982 blocks. The
window contains 6,617 raw rows, 6,618 observer rows, 75,571 isolated probe
metric rows, 133 reconciliation rows, and 26 finalized rows. Both evidence
directories are mode 0700 and every contained file is mode 0600.

The latency values below come only from the live warm-state observer records.
The post-window collector replay corrects Pons event-generation classification
but does not replace or fabricate observation latency.

| Launchpad | Truth | Confirmed | False positives | Missed (detector / coverage) | Live latency p50 / p95 / p99 | Quote result | Profile evidence |
|---|---:|---:|---:|---:|---|---|---|
| Bow | 0 | 0 | 0 | 0 (0 / 0) | - | no activity | no activity |
| LaunchHood V3 | 4 | 4 | 0 | 0 (0 / 0) | 0.419 / 1.641 / 1.641 ms | 4/4 matched; zero identity, action, direction, prediction, or quote errors | embedded buy 4 |
| Clanker | 6 | 6 | 0 | 0 (0 / 0) | 0.341 / 4.809 / 4.809 ms | 6/6 matched; zero identity, action, direction, prediction, or quote errors | extensionless 1; pinned extension 5 |
| Bankr/Doppler | 4 | 4 | 0 | 0 (0 / 0) | 0.522 / 2.579 / 2.579 ms | 4/4 matched; zero identity, action, direction, prediction, or quote errors | V3/ERC-7579 4 |
| Pons | 40 | 39 | 0 | 1 (1 / 0) | 0.346 / 1.475 / 1.957 ms | one current-generation row quote-blocked; 39 legacy rows not applicable | one hard detector miss |
| Hood | 0 | 0 | 0 | 0 (0 / 0) | - | no activity | no activity |

Bankr emitted ten claims: four confirmed observations, four reverted attempts,
and two out-of-scope observations. Pons emitted 45 claims: 39 confirmed legacy
observations and six out-of-scope observations. These reverted and
out-of-range rows are not false positives.

The 14 finalized paper plans comprise four LaunchHood plans at 9,801 bps
simulated immediate round-trip return, six Clanker plans at 849 bps, and four
Bankr plans at 97 bps. Every plan includes independent entry sizing, entry and
exit slippage, a full-position exit quote and exit policy; every plan retains
`execution_eligible: false` and `broadcast: false`.

The exact Pons miss is transaction
`0x7a13c94f90ddaa7d35d639f046f30a44d1d9b5fe449550fd0b75e5e65a0fb4c6`.
It emitted a current-generation Pons event inside an unreviewed EIP-7702
self-call using selector `0x3f707e6b`. At receipt block `11777530`, the account
designator was
`0xef0100dc44136e7ca3509a73fc6c22b6a6bd302bf9a1e2`, delegating to
`0xdc44136e7ca3509a73fc6c22b6a6bd302bf9a1e2`; the delegated runtime hash was
`0x6d7379e6220b87ceeade4a4e069c6a5ca4636fc228a0c948a0c87177860f3baa`.
None is an independently reviewed production pin or per-account execution
profile.

Commit `0f8dab8` corrects collector classification by deriving Pons generation
from the receipt event, rather than whether the observer happened to claim the
transaction. The corrected replay at
`.runtime/paper-session-20260716-d00adac-clean4-post-pons-generation-replay`
therefore classifies the missed row as current generation, while intentionally
preserving the observer miss and rejecting its quote with the invalid-envelope
blocker. This is a classification correction, not support for the wrapper.
Candidate provenance work is still under review and is not integrated or
claimed as expected-pin authority. The designator, delegated runtime, and
per-account execute profile all require independent pin review before this
envelope can be supported or promoted.

Clean4 establishes one of the three required current-code live windows. Its
standalone readiness counts are LaunchHood 4, Clanker 6, Bankr 4, Pons 0,
Bow 0, and Hood 0. Every launchpad remains `paper_evidence_ready: false`,
`authorizes_canary: false`, and `execution_eligible: false`; Pons additionally
has the hard detector miss. Clean4 is deliberately not added blindly to the
historical replay aggregate below because that aggregate mixes older live code
and replay-only cohorts with different evidence boundaries.

## Clean5 exclusion and clean6 completed live evidence

Clean5 is excluded. Its direct feed reported a transient WebSocket reset
without a closing handshake, followed by `read_error`, `disconnected`, one
reconnect, and a later `coverage_closed`. The run retained only `.partial`
anchors and stream artifacts; it has no completion manifest, reconciliation,
finalized evidence, or readiness decision. This is the intended fail-closed
disposition: none of clean5's 7,987 raw rows is scored, used for latency, or
counted toward readiness, and the partial evidence remains untouched.

Clean6 is a completed live session at
`.runtime/paper-session-20260716-e975555-clean6`. Its schema-1 completion
manifest binds the exact expected pins, startup snapshot, executables, anchors,
raw stream, observer output, independent reconciliation, and finalized output.
The snapshot boundary is L2 block `11825120`, hash
`0xb197966685eeef043e1db2e137266e6d1689f77f843c402b721b7c606d8a1476`,
L1 block `25549949`, timestamp `1784261757`, with 39 observed pins. The start
boundary is block `11825238`, hash
`0x84c4c9043e8bbc95e41e47e5422ac897b4fa090ab5df250752fc7a7ac35ce13a`;
the cutoff is block `11831205`, hash
`0x6f67bbea8d056a47fc1534b8ae08a697700366f44a54e911d492560e43eb3742`.
The 118-block snapshot-to-start gap is within the manifest's 500-block limit.
The complete scored interval is `11825239..=11831205`, or 5,967 blocks.

Clean6 contains 6,817 raw rows, 6,818 observer rows, 69,656 isolated probe
metric rows, 116 reconciliation rows, and 28 finalized rows. Probe state was
exactly `connected` then `coverage_closed`, with zero reconnects. The directory
is mode 0700 and every contained artifact is mode 0600.

| Launchpad | Truth | Confirmed | False positives | Missed (detector / coverage) | Live latency p50 / p95 / p99 | Quote result | Profile evidence |
|---|---:|---:|---:|---:|---|---|---|
| Bow | 0 | 0 | 0 | 0 (0 / 0) | - | no activity | payable 0; zero-buy 0 |
| LaunchHood V3 | 2 | 2 | 0 | 0 (0 / 0) | 0.242 / 0.611 / 0.611 ms | 2/2 available and matched; zero identity, action, direction, prediction, or quote errors | embedded buy 2 |
| Clanker | 5 | 5 | 0 | 0 (0 / 0) | 0.498 / 0.632 / 0.632 ms | 5/5 available and matched; zero identity, action, direction, prediction, or quote errors | extensionless 0; pinned extension 5 |
| Bankr/Doppler | 10 | 9 | 0 | 1 (1 / 0) | 0.530 / 1.510 / 1.510 ms | 9 available and matched; one truth row blocked; zero identity, action, direction, prediction, or quote mismatches on eligible rows | V3 9; ERC-7579 9; V1/V2/direct 0 |
| Pons | 29 | 29 | 0 | 0 (0 / 0) | 0.393 / 3.289 / 5.324 ms | 29 legacy/not applicable; zero current-generation quote-eligible rows | current generation 0 |
| Hood | 0 | 0 | 0 | 0 (0 / 0) | - | no in-scope activity; one claim was out of scope, not a false positive | current curve 0 |

Bankr emitted 17 claims: nine confirmations and eight reverted attempts. Pons
emitted 35 claims: 29 confirmed legacy observations and six out-of-scope
observations. Across the six scored launchpads, false positives, feed-coverage
misses, identity mismatches, direction mismatches, prediction mismatches, and
independent-quote mismatches are all zero. The one Bankr detector miss is the
only hard scoring error.

The exact Bankr miss is transaction
`0xc85b51ecb810158b02511586552295fc26e2720764a9b4a4a9a9cda774efdc20`
at block `11828501`. Independent review classifies it as a supported Bankr V4
`curve_ticks_v3` launch inside the reviewed ERC-4337/ERC-7579 envelope. The
observer rejected it because its strict receipt validator assumed the creator
beneficiary must be first, while this launch's canonical address ordering put
the protocol beneficiary first. The bounded fix derives beneficiary roles by
identity and share while preserving canonical address ordering, then binds
token vesting to the derived creator. As of this report commit, that fix exists
only in an isolated worktree: it is not integrated or pushed, its tests are
pending, and clean6 retains the detector miss and blocked quote. This
provisional disposition does not authorize support or promotion.

Clean6 finalized 16 independent paper plans: two LaunchHood, five Clanker, and
nine Bankr. Their simulated immediate round-trip returns are 9,801, 849, and
97 bps respectively. Every plan includes an independent entry quote and
slippage minimum, a full-position exit quote and minimum, and the exit policy;
both the top-level and nested exit-plan `execution_eligible` and `broadcast`
flags are `false` for all 16 plans.

The manifest-trusted clean6 readiness decision uses one submitted, complete,
independent live window. Policy requires 100 quote-eligible confirmations per
launchpad, ten observations per supported profile/envelope, three independent
complete windows, and zero false positives, detector misses, identity,
direction, prediction, or quote mismatches:

| Launchpad | Quote eligible / 100 | Profile observations / 10 | Complete windows / 3 | Hard error |
|---|---:|---|---:|---|
| Bow | 0 | payable 0; zero-buy 0 | 1 | none; no samples |
| LaunchHood V3 | 2 | embedded buy 2 | 1 | none |
| Clanker | 5 | extensionless 0; pinned extension 5 | 1 | none |
| Bankr/Doppler | 9 | V1 0; V2 0; V3 9; direct 0; ERC-7579 9 | 1 | one detector miss; policy maximum 0 |
| Pons | 0 | current generation 0 | 1 | none; legacy rows do not qualify |
| Hood | 0 | current curve 0 | 1 | none; no samples |

Every clean6 readiness row remains `paper_evidence_ready: false`,
`authorizes_canary: false`, and `execution_eligible: false`. Clean6 is not
combined blindly with older live or replay cohorts whose completion and binary
provenance do not satisfy the current manifest-trust boundary.

## Historical replay aggregate readiness decision

The conservative evaluator accepts the four non-overlapping anchored windows
as complete scoring inputs, but every launchpad remains not ready and every
row retains `authorizes_canary: false` and `execution_eligible: false`:

| Launchpad | Quote-eligible / 100 | Supported-envelope progress | Remaining hard errors |
|---|---:|---|---|
| Bow | 0 | payable 0/10; zero-buy 0/10 | none; no samples |
| LaunchHood V3 | 7 | embedded buy 7/10 | none |
| Clanker | 16 | extensionless 1/10; pinned extension 15/10 | none; Tator row excluded as unsupported |
| Bankr/Doppler | 18 | V3 18/10; ERC-7579 18/10; V1 0/10; V2 0/10; direct 0/10 | none |
| Pons | 1 | current generation 1/10 | two identity mismatches on the current row |
| Hood | 2 | current curve 2/10 | none |

This historical four-window count does not replace the operational live
requirement:
fresh1 may be build-contaminated, quiet1 is a replay, and clean1's live binary
predated the long-name fix. Clean2 is build-idle and supplies useful live
latency, but its binary predates the active-boundary fix. A new live window on
exact `bbef8b4` or later was therefore required. Clean4 now supplies the first
such complete current-code live-only cohort, but it is scored separately above
and does not retroactively make these older cohorts promotion-grade. Passing
one Clanker profile threshold does not override the 100-confirmation threshold,
the three-current-window requirement, the sparse extensionless profile, or the
clean4 Pons detector miss.

## Artifact integrity

The key local files are bound by these SHA-256 digests:

| Artifact | SHA-256 |
|---|---|
| fresh 39-pin startup snapshot | `939163c21386227ecb516bdef404fe2b3f483bac076338656593cf0f5d164fad` |
| fresh1 raw feed | `fdbd58e36cf55cb222181b528322082529ccedf05b88d2e8567a11f2363b230c` |
| fresh1 observer output | `f59884a592f6f955ab729b8ec2e8e3df06ef82bea83de9ffdc663185724e49a8` |
| fresh1 final-code observer output | `7524757e59255d0575f6558af6d0abaf42fafc96048311789b4a5ecbecfb3659` |
| fresh1 final-code reconciliation | `d8031e6e96d6898209fe38b6d2cd9df9b5a3274968c05d6b4a26844f0c4d05ae` |
| fresh1 final-code finalized output | `e516e60e6a03e94c25568df87e94203a4ded1c9099db82dd719676655955275d` |
| quiet1 final-code observer output | `3210e2382426c9f67066d472950217342124ea0f407a08ce97a6906ae5c4629d` |
| quiet1 final-code reconciliation | `9d55a11479a9daac48c9f88e6fc95b78ba80b9a64b3852b320bd3c96c9f1e200` |
| quiet1 final-code finalized output | `8518232a19825ba301b66fa3d54fd0fbc038bb03e58fb8b3e0ebc5c5f018c5f0` |
| clean1 raw feed | `d6742cad3be3e839151b98b8aea6ecbd270d09fed20dc94ae7151ae280d6d5fc` |
| clean1 live observer output | `ccc0630f11c66c0efe7463e808e2b900e38bcbdc12a5eced8ad40aa0657c52ab` |
| clean1 live reconciliation | `0a04f013bcf89ed1691c0d892aea00e8b70cf3bb977b2efcc77f788782a465d6` |
| clean1 live finalized output | `755640cc2456f1e426e4c544304f65723f4e9b7f6f183a660c508d7f70bbf923` |
| clean1 post-fix observer replay | `c16e13253ccfbe8560fcdf28d257588fd8b5c6e7af1ac510618ec17b67888d6a` |
| clean1 post-fix reconciliation | `b689dcdb5e8bc09e0864bd32b392267ebf2b6dfd69c3b73c873ef29a98bb3d1c` |
| clean1 post-fix finalized output | `dbff28be3744e2121c5b8d8f24b89b6522dd5718e265d419316e7e3765bfa559` |
| clean2 39-pin startup snapshot | `d10a0c9d2aae451c63badb2a111dc5c46c9de2c6dfdec3c5f4ac0b4f80407dc1` |
| clean2 raw feed | `dcb4b021636e38b11047e561d2a114aaf4b3716720116ddda32867a8b51461ed` |
| clean2 live observer output | `b30a3c241329e12e930d95877552f0bf9984e78285cc02e74240bfa418c9644a` |
| clean2 live reconciliation | `0ba5f898edc17f5e232079acd35fc4f2864c7798fd7365bce215b8747ba009dd` |
| clean2 live finalized output | `f8f50a323b012c79b3e95cb7f4255ec14bd6a32c45ff9736e7f90c7003946efb` |
| clean2 post-fix observer replay | `e0e4adeb3d8b898e980c4291f9e79a76b7fc6bd34fd4f97491dc4214f5a7ad68` |
| clean2 post-fix reconciliation | `3f452cecc35d2a661e917f09f7e62fdd6e2be6450a994596def857cc5c597f40` |
| clean2 post-fix finalized output | `8301ac9c478da600a596ef0fb8dc24cb8f3d01bf08518cd7d9e5ae4dc40acd83` |
| four-window aggregate readiness decision | `60c80b796393aa38ed2d6af3cc05c7b515fd1069568d7fe013d6740600a97abd` |
| clean4 39-pin startup snapshot | `093180b4a1f339ddd65cb1b27af1c059e7d2897cb2533edc38f1a18695967592` |
| clean4 raw feed | `445c6221facff2418bdca9ff24375f694d26bbaa0509c19bc73914a90c269fae` |
| clean4 live observer output | `0217b91ec8d14950cc4263c401bfbd43e90f131c11439efc36864fbbb45e73fc` |
| clean4 probe metrics | `fa9db668daba5c08d30b21f42471c49d1c0139c86360b70b41de933f401d7aff` |
| clean4 live reconciliation | `a1e7d8e8b40d5a574fa99f983cefc9b634d029e67e6ec17e22899335d5a5d1bb` |
| clean4 live finalized output | `b39447733c727fc8255d59a5ed98a8755d74d272e159a6a7f27b4cc40ee77c97` |
| clean4 corrected-generation reconciliation | `314d7bc6f80f56b0f133ed9eef846fba87c3423c46bb731f85ae88cd63c58946` |
| clean4 corrected-generation finalized output | `a01cad25800d648aba56e8bc5e525ed21831686c205dcd4528b2a2a050389546` |
| clean6 39-pin startup snapshot | `0cb94e5fe0e8191efe72bf45941ef81f59b7b92ac913a8eeeecb27778b14fc50` |
| clean6 raw feed | `8e5746208e0acc12c9feae3f317869afac04880729032115466bee31ccff2ec9` |
| clean6 live observer output | `a5db86fd1270bd7fa273455c1926953b5925a684ef6b1199b01f45c28a8fb915` |
| clean6 probe metrics | `6b59401fb0fb009327e170456b976a34c5dd65966afa55e7ef023bd1067c1e21` |
| clean6 live reconciliation | `2206306943e15f17db55543e7437953641a63d14f61da21cd2ffff99c4accfac` |
| clean6 live finalized output | `d49ff708445a6095b1fafc7f9e55ea364f5cb0d5d71cb5c8aab3e0357750e8f4` |
| clean6 completion manifest | `19cc64b36e7166a4b1f0365606675792e9851e1d270de34d26ca3fd8d0eb30fb` |
| clean6 readiness decision | `8da7d5c09205acfeca47914d0d9eb82870350aa52189ab1a0eb5faf3127790f6` |

## Remaining evidence gaps

No launchpad is ready for promotion. The fixed policy requires 100
quote-eligible confirmations, ten per supported profile/envelope, and three
complete non-overlapping windows, with zero false positives, detector misses,
identity failures, direction failures, prediction failures, or quote failures.
Current gaps include:

1. Clean6 is the first live window accepted by the current completion-manifest
   trust boundary. Accumulate two more complete, non-overlapping,
   provenance-compatible live windows, not replay-only scores. Retain both the
   historical clean4 Pons miss and the clean6 Bankr miss as hard errors in
   their respective evidence cohorts.
2. Observe both Clanker
   `extensionless_single_position` and `pinned_extension_five_position` at
   least ten times each. The pinned-extension profile has 15 observations, but
   extensionless remains at one. Keep the payable Tator-extension/two-position
   envelope quote-blocked pending separate pin and semantic review.
3. Keep the clean6 Bankr beneficiary-order fix unpromoted until its exact real
   proof and strict negative tests pass and the bounded change is integrated.
   Expand Bankr beyond V3/ERC-7579: V1, V2, direct Airlock, and the remaining
   per-stratum thresholds are still sparse or absent.
4. Keep the 37 fresh1 legacy Pons confirmations discovery-only; they are not a
   promotion profile. Collect current-generation observations with strict
   token/pool identity and quote validation instead.
5. Expand Hood beyond two final-code confirmations; two quotes cannot cover
   the current-curve profile or the 100-confirmation threshold.
6. Gather Bow activity for both payable and zero-initial-buy profiles. Zero
   events are not evidence of recall.
7. Keep Flap discovery-only until its direction, identity, sizing, slippage,
   exit, and migration semantics are complete.

Canary authorization remains false. Any one-wallet, one-trigger, tiny-amount
canary requires separate explicit approval to resume Droplet work.
