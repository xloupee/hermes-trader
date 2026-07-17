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
- 3,654 raw-feed rows, 3,655 observer rows, 122 V3-replay reconciliation
  rows, and 21 finalized rows; and
- every persisted artifact has mode 0600.

Collection used the intended local topology: `hermes-feed probe` wrote the
raw stream to a private FIFO, `tee` persisted `raw-feed.jsonl` while forwarding
the same bytes, and `hermes-launchpad-paper --input -` consumed stdin. Probe
stdout was isolated in `probe-metrics.jsonl`, not mixed into the raw stream.

The finalized V3 replay is the authoritative scored form of this window:

- `.runtime/paper-session-20260716-pins39-fresh1/reconciliation-evidence-v3replay.jsonl`
- `.runtime/paper-session-20260716-pins39-fresh1/launchpad-paper-finalized-v3replay.jsonl`

The replay corrected independent V3 quote reconstruction; it did not replace
the observer stream. The latency fields still come from the original live
observer records. Builds may have overlapped the fresh1 collection, however,
so those latency percentiles are **potentially build-contaminated diagnostics**
and are not promotion-grade warm-state latency evidence.

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
| Hood | 2 | 1 | 0 | 1 (1 / 0) | 7.374 / 7.374 / 7.374 ms | token missing 1; 0 wrong values | 1/1; 0 mismatch | entry 1/1, exit 1/1; 0 mismatch | 1/1; 0 mismatch; 1 blocked truth |

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
no readiness decision. Hood has one missing token prediction and one
complete-coverage detector miss. Bow and Clanker had no fresh1 truth at all.

## Fresh1 paper sizing, slippage, and exits

Every finalized plan independently applies a fixed `0.001 WETH` entry, 1%
entry slippage, a full-position immediate exit, and 1% exit slippage. Leader
amounts are not reused.

| Launchpad | Plans | Expected entry output | Entry minimum | Expected full exit | Exit minimum | Simulated round-trip return |
|---|---:|---:|---:|---:|---:|---:|
| LaunchHood V3 | 1 | `729729.244852920426979991` tokens | `722431.952404391222710191` | `0.000980107152180750 WETH` | `0.000970306080658942 WETH` | 9,801 bps |
| Bankr/Doppler | 7 | `904475.731970933213275501` or `904475.731970933213278599` tokens | `895430.974651223881142745` or `895430.974651223881145813` | `0.000009736782115644 WETH` | `0.000009639414294487 WETH` | 97 bps |
| Hood | 1 | `388879.762303340027827257` tokens | `384990.964680306627548984` | `0.000980100000000000 WETH` | `0.000970299000000000 WETH` | 9,801 bps |

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

The complete post-detector-fix reconciliation is in
`.runtime/paper-session-20260716-quiet1-replay-fixed2`:

| Launchpad | Truth | Confirmed | False positives | Missed | Out of scope / reverted | Independently matched quotes | Identity limitation in this artifact |
|---|---:|---:|---:|---:|---:|---:|---|
| Bow | 0 | 0 | 0 | 0 | 0 / 0 | 0 | no activity |
| LaunchHood V3 | 3 | 3 | 0 | 0 | 0 / 0 | 3/3 | token 3/3 and pool 3/3 matched |
| Clanker | 3 | 3 | 0 | 0 | 1 / 0 | 3/3 | token prediction missing 3; pool-ID scoring was not yet present |
| Bankr/Doppler | 2 | 2 | 0 | 0 | 4 / 2 | 2/2 | token prediction missing 2; pool-ID scoring was not yet present |
| Pons | 28 | 28 | 0 | 0 | 12 / 0 | 1/1 current-generation quote; 27 legacy not applicable | pre-fix artifact counts all 28 missing; corrected scope excludes 27 exact legacy rows, while the one current row remains identity-incomplete |
| Hood | 0 | 0 | 0 | 0 | 0 / 0 | 0 | no activity |

All eligible action, entry-direction, exit-direction, and independent quote
checks in fixed2 matched. Its nine finalized plans consisted of three
LaunchHood, three Clanker, two Bankr, and one current-generation Pons plan.
The fixed tiny-policy outcomes were:

| Launchpad | Expected entry output | Entry minimum | Expected full exit | Exit minimum | Return |
|---|---:|---:|---:|---:|---:|
| LaunchHood V3 | `729729.244852920426979991` | `722431.952404391222710191` | `0.000980107152180750 WETH` | `0.000970306080658942 WETH` | 9,801 bps |
| Clanker | `2977989.243985511988125632` | `2948209.351545656868244375` | `0.000084919129125191 WETH` | `0.000084069937833939 WETH` | 849 bps |
| Bankr/Doppler | same two entry variants as fresh1 | same two minima as fresh1 | `0.000009736782115644 WETH` | `0.000009639414294487 WETH` | 97 bps |
| Pons current generation | `663271.426017223774908828` | `656638.711757051537159739` | `0.000980106818839881 WETH` | `0.000970305750651482 WETH` | 9,801 bps |

The later directory
`.runtime/paper-session-20260716-quiet1-replay-identity` is a **partial identity
replay, not a complete replacement score**. Its observer file contains three
LaunchHood claims with three token and three pool predictions, 40 Pons claims,
and 48 Flap discovery claims, but no Clanker or Bankr claims. Its finalized
metrics consequently still report three Clanker and two Bankr detector misses.
It proves the LaunchHood identity fields in that replay; it does not prove a
single unified post-identity quiet1 run for Clanker or Bankr. Fresh1 separately
proves Bankr token and V4 pool-ID prediction 7/7. Clanker had no truth in
fresh1, so a unified Clanker post-identity live/replay sample remains missing.

## Artifact integrity

The key local files are bound by these SHA-256 digests:

| Artifact | SHA-256 |
|---|---|
| fresh 39-pin startup snapshot | `939163c21386227ecb516bdef404fe2b3f483bac076338656593cf0f5d164fad` |
| fresh1 raw feed | `fdbd58e36cf55cb222181b528322082529ccedf05b88d2e8567a11f2363b230c` |
| fresh1 observer output | `f59884a592f6f955ab729b8ec2e8e3df06ef82bea83de9ffdc663185724e49a8` |
| fresh1 V3-replay reconciliation | `099a953b9a9c8443da69bd16835de8015e3d1b879b61298b393be086139ba799` |
| fresh1 V3-replay finalized output | `53980dc7f4f233bc33201d3710d009aa353b284d426b99577f83d47effb80060` |
| quiet1 fixed2 observer output | `da374e8b618497c381ba53682d5d835fd4207884b3ac02600b76c9697a23c31e` |
| quiet1 fixed2 reconciliation | `9ea0112d249abe723c98a9f382049c1f14f181d164cf4f4f75881de5e904bfa1` |
| quiet1 fixed2 finalized output | `9d3404772346cc28316d7817977704fb649d2ee92872d7ec03f9f1b0c5da47d6` |
| quiet1 partial identity observer output | `fee4bdff56fffd352f0ae566502ebb7aa92a289fb62b204d1a9cc1500c70a14e` |
| quiet1 partial identity finalized output | `38523bd30a8d9d64640b95ea814420b594b4d9e842e34539e82d2436e47a410f` |

## Remaining evidence gaps

No launchpad is ready for promotion. The fixed policy requires 100
quote-eligible confirmations, ten per supported profile/envelope, and three
complete non-overlapping windows, with zero false positives, detector misses,
identity failures, direction failures, prediction failures, or quote failures.
Current gaps include:

1. Collect clean, build-idle warm-state windows; fresh1 latency must be
   repeated before its percentiles can be used.
2. Produce a unified post-identity Clanker replay/live score and observe both
   `extensionless_single_position` and `pinned_extension_five_position` at
   least ten times each.
3. Expand Bankr beyond seven V3/ERC-7579 confirmations. V1, V2, direct Airlock,
   and the remaining per-stratum thresholds are unproven in fresh1.
4. Keep the 37 fresh1 legacy Pons confirmations discovery-only; they are not a
   promotion profile. Collect current-generation observations with strict
   token/pool identity and quote validation instead.
5. Fix and negatively test the Hood miss, then add pre-receipt token identity;
   one confirmed quote cannot cover the current-curve profile.
6. Gather Bow activity for both payable and zero-initial-buy profiles and
   obtain any fresh Clanker activity. Zero events are not evidence of recall.
7. Keep Flap discovery-only until its direction, identity, sizing, slippage,
   exit, and migration semantics are complete.

Canary authorization remains false. Any one-wallet, one-trigger, tiny-amount
canary requires separate explicit approval to resume Droplet work.
