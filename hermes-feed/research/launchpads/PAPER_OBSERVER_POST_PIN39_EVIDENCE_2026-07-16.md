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

## Aggregate readiness decision

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

The four-window count does not replace the operational live requirement:
fresh1 may be build-contaminated, quiet1 is a replay, and clean1's live binary
predated the long-name fix. Clean2 is build-idle and supplies useful live
latency, but its binary predates the active-boundary fix. A new live window on
exact `bbef8b4` or later is still required before any window can be called
current-code promotion-grade. Passing one Clanker profile threshold does not
override the 100-confirmation threshold or the sparse extensionless profile.

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

## Remaining evidence gaps

No launchpad is ready for promotion. The fixed policy requires 100
quote-eligible confirmations, ten per supported profile/envelope, and three
complete non-overlapping windows, with zero false positives, detector misses,
identity failures, direction failures, prediction failures, or quote failures.
Current gaps include:

1. Collect a new clean, build-idle warm-state window on exact `bbef8b4` or later;
   then accumulate three promotion-grade live windows, not replay-only scores.
2. Observe both Clanker
   `extensionless_single_position` and `pinned_extension_five_position` at
   least ten times each. The pinned-extension profile has 15 observations, but
   extensionless remains at one. Keep the payable Tator-extension/two-position
   envelope quote-blocked pending separate pin and semantic review.
3. Expand Bankr beyond the observed V3/ERC-7579 confirmations. V1, V2, direct
   Airlock, and the remaining per-stratum thresholds are still sparse or
   absent.
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
