# Final-tuple three-window paper campaign

Evidence date: 2026-07-16 UTC. Network: Robinhood Chain mainnet, chain ID
`4663`. Report base: `6e21e69ae15d80a16a15b22fc5680e2ac2efe94e`
(`agent/local-bankr-paper-observer`). Artifact root:
`/Users/kennethjiang/Documents/hermes-trader 2/hermes-feed/.runtime/paper-campaign-finaltuple-20260716`.

## Decision

The frozen campaign is internally consistent and independently reproducible:
three complete, independent, non-overlapping live-acquisition windows were
accepted, zero partial windows were present or excluded, all 27
manifest-listed artifacts matched their recorded byte sizes and Keccak-256
digests, and the pinned readiness and evidence-report executables regenerated
both authoritative JSONL outputs byte-for-byte.

This is paper evidence only. For every launchpad,
`paper_evidence_ready=false`, `authorizes_canary=false`, and
`execution_eligible=false`. No Droplet, server, wallet, key, signing,
broadcast, execution, deployment, or canary work occurred. Nothing in this
report authorizes any of those actions.

The report checkout is exactly at the requested Git commit. The campaign
artifacts do not themselves embed a Git commit: they attest the binaries,
orchestration inputs, snapshots, manifests, and outputs by content digest.
The Git SHA and the campaign digest tuple are therefore separate provenance
claims.

## Locked provenance

Expected-pins content Keccak-256:
`0x3a192547af7a76a47d5de1b8102da0dc0bb77b7047a53c855b7d264aefa1a34c`.

| Component | Campaign-locked Keccak-256 |
| --- | --- |
| `hermes-feed` | `0x3a4ed7a3036399088140dbbafa1e7a5b3d21e9e70435f9c6e128e87dca79a154` |
| observer/finalizer `hermes-launchpad-paper` | `0x219fdaeb12d6504de5d12cd659afe538bf67ff288dc62b1760e773866bc097d9` |
| `hermes-launchpad-reconcile` | `0x7337e98524a32a5a8d6085d5aa2a0e79540775c469ae0335325dd73e36c9db76` |
| `hermes-launchpad-chain-head` | `0xf03c71c84ac7968f4222af98acf826b5e3796d7520851c10f84e10d9c0840ec5` |
| `hermes-launchpad-readiness` | `0x1b125762677439042bac436f37b9b60ce5b0273d641640845e035d5075118f8a` |
| `hermes-launchpad-evidence-report` | `0x116f9463f9d1622b386cdf5c6e0dac628c3ec04512f7e104d54117dc15a5a912` |
| `hermes-launchpad-pin-snapshot` | `0x0b0ff5ee296e1ef47da19a57e6de04c5e815f72cdec56ba352aaa158f73a1d8b` |
| `scripts/run-launchpad-paper-local.sh` | `0xb065b10aa70b8caaa586298346ac06d11f27871dd6c35e0e6f3249e876356fa3` |

The campaign-lock content hash is
`0x2a2a259c0c21397f61c9ffc1c32f73ee32700fcc4dc3b6dfe98286f2070b5a53`.
The authoritative-readiness output hash is
`0xeecd48c7edb3a4c264f0457e5d1817f75f0ffba7f68ec8c20aa6e5edde503030`;
the final evidence-report output hash is
`0x9450dec5c8cdf24dc3d2ae76d61d91f3ddef4af0bab560702c914cb137f016cd`.

The current `scripts/run-launchpad-paper-campaign.sh` is byte-identical in the
report and authoritative-source checkouts and hashes to
`0x11c708fbb10e0f9d77659517762d035c4e1d32d271e5fde8ed275b582b24a7c1`.
That outer-runner hash is **not** recorded in `campaign-lock.json`, so it is a
current-source verification, not a frozen campaign attestation.

## Accepted windows

All three snapshot reports say `chain_id=4663`, `pin_count=41`, and
`expected_validation_passed=true`. The allowed snapshot-to-start gap is 500
L2 blocks; the observed gaps were 62, 73, and 74.

| Window | Snapshot identity | Scored coverage and boundary identity | Content identities |
| --- | --- | --- | --- |
| A | L2 `11922012`, `0xae6e83b9699f323ed169aafd2586885632593b493120726dfc62792b139d9592`; L1 `25550759`; timestamp `1784271483` | start L2 `11922074`, `0x4dad09c88284c3e5e41be6f0f41b262e69ad347d60952ea17167422b9dd6c758`; coverage `[11922075,11925060]`; cutoff `0x915ccb7ff7a073d0ffeafd0a679f4f5add4bd7c8006e1832bb4bc5cf0cf21522` | snapshot `0x7e89b6e84bcb36b146bd5e3cd3008a29c63594b47a13947551e07caee57ba2ab`; snapshot report `0x01f83dab90b68e22669f7eed3a83eb6812c50270b32cba2998b846fc19629491`; manifest `0x8c475da476cad10d25f802bdafd3b243dd2be5ee6541cefd3ecc282d57f711dd`; finalized rows `0x4e9000fc2bab0c923e1177f84d6ca83ccbdd11c231a523c0c9deb0cc1e17be2d` |
| B | L2 `11925449`, `0x52985d19f666d0225289abe21cda54d3666d40a31cf3816e7c8f2b2fbe4b34e7`; L1 `25550788`; timestamp `1784271827` | start L2 `11925522`, `0x6be63083d3c6e9f00eaeb4853467674405b04776bb38dd0919978ebf8b31dccc`; coverage `[11925523,11928502]`; cutoff `0x051f4ddad86e400658693fa27332f7ca3d9c745836db6e45fcb38ff7c5b28de5` | snapshot `0xeadfcfbb7a0f3026db348dadadfbedb6ac491767842736e2a801446e2f90f297`; snapshot report `0x0bb577d84ca30f3cef86f46926fb0b18990cfb0a7ea9b62071aa63ee15afe95d`; manifest `0xacc34ced382bb1a39071001486f2d358b37c630aac97d1ac09f5a6e2bb66f21b`; finalized rows `0x06a5e1ccc6731eec8616bcf285334cd89131670389d7ca74e0050e9dc843ad71` |
| C | L2 `11928827`, `0x9b345eb35e7f601de1f72ee4897823fd32d2a194932117f0500b99d0af97dd7d`; L1 `25550816`; timestamp `1784272167` | start L2 `11928901`, `0x2a49e7a38efae70ecd9471c799fd9af1d1e50ecdb62ca58f702daa8faf5de3e5`; coverage `[11928902,11931893]`; cutoff `0xc6afccead2a1302554c1801e4b9a30fa7d0d6adb27f2549cc89b70dd7553cd1f` | snapshot `0xeb0bfd5b5236c5f17038ab9ec67d56facb5e32dfe7009792ae64ec15484f7984`; snapshot report `0xb877d6ecfb2b411c931acbba729c8356c0c0547683c3add01c3f8306435d11a7`; manifest `0x8fab71c3f9208e758b5215dcc5af88b0d772ff4330b725ea4e3038168adb6ad2`; finalized rows `0xf78a9d4f38912f07937a285faf602ba9bc07de29ceaddecef40be7df1b1b97ec` |

The finalized files contain 20, 19, and 19 rows respectively: six
reconciliation-metric rows and six readiness rows in each, plus 8, 7, and 7
paper-plan rows. Each completion manifest says `completed=true` and
`acquisition=live`. There are exactly three window directories and no
`*.partial`, partial-session, FIFO, symlink, or extra-window artifact in the
campaign root. Aggregate `accepted_window_count=3` and
`excluded_partial_window_count=0`.

## Quantitative result

`eligible` below means quote-eligible confirmed paper evidence, not execution
eligibility. Latencies are exact nanoseconds from the campaign aggregate.
Every row has zero false positives, feed-coverage misses, identity mismatches,
direction mismatches, prediction mismatches, and independent-quote
mismatches.

| Launchpad | Confirmed / eligible | Latency p50 / p95 / p99 (ns) | FP / detector miss / identity / direction / prediction / quote mismatch | Entry / exit plans | Supported-profile observations | Slippage and simulated immediate round trip |
| --- | ---: | ---: | ---: | ---: | --- | --- |
| Bow | `0 / 0` | `null / null / null` | `0 / 0 / 0 / 0 / 0 / 0` | `0 / 0` | `payable_initial_buy=0`; `zero_initial_buy=0` | no samples |
| LaunchHood V3 | `9 / 9` | `290000 / 690000 / 690000` | `0 / 0 / 0 / 0 / 0 / 0` | `9 / 9` | `embedded_initial_buy=9` | all entry and exit plans use `100` bps (`0x64`); return sample count `9`, min/p50/p95/p99/max all `9801` bps (`0x2649`) |
| Clanker | `4 / 4` | `298000 / 671000 / 671000` | `0 / 0 / 0 / 0 / 0 / 0` | `4 / 4` | `extensionless_single_position=3`; `pinned_extension_five_position=1` | all entry and exit plans use `100` bps; return sample count `4`, min/p50/p95/p99/max all `849` bps (`0x351`) |
| Bankr/Doppler | `0 / 0` | `null / null / null` | `0 / 16 / 0 / 0 / 0 / 0` | `0 / 0` | `curve_ticks_v1=0`; `curve_ticks_v2=0`; `curve_ticks_v3=0`; `direct_airlock=0`; `erc7579=0` | no admitted plans or round-trip samples |
| Pons | `59 / 0` | `314000 / 780000 / 1183000` | `0 / 0 / 0 / 0 / 0 / 0` | `0 / 0` | `current_generation=0` | no quote-eligible plans or round-trip samples |
| hood.fun | `9 / 9` | `636000 / 1142000 / 1142000` | `0 / 2 / 0 / 0 / 0 / 0` | `9 / 9` | `current_curve=9` | all entry and exit plans use `100` bps; return sample count `9`, min/p50/p95/p99/max all `9801` bps (`0x2649`) |

The simulated round-trip field is a paper-quote return ratio
`exit_output * 10000 / entry_input`, not realized profit or loss: `9801` means
98.01% of the input is returned and `849` means 8.49% is returned.

Per-window confirmed counts were: LaunchHood V3 `2/3/4`, Clanker `2/0/2`,
Pons `21/22/16`, hood.fun `4/4/1`, Bow `0/0/0`, and Bankr/Doppler
`0/0/0` for A/B/C. Per-window detector misses were Bankr `9/4/3` and Hood
`1/1/0`; all other launchpad/window miss counts were zero. The Pons counts
are confirmed legacy/not-applicable observations; none entered the
`current_generation` quote-eligible profile.

### Readiness failures

The policy requires at least 100 quote-eligible confirmations, at least 10
observations in every supported profile envelope, three independent complete
windows, and zero error counters. The three-window requirement passed for all
launchpads. The remaining failures are:

- Bow: quote-eligible `0 < 100`; both supported profiles `0 < 10`.
- LaunchHood V3: quote-eligible `9 < 100`; embedded profile `9 < 10`.
- Clanker: quote-eligible `4 < 100`; extensionless profile `3 < 10`; pinned
  extension profile `1 < 10`.
- Bankr/Doppler: quote-eligible `0 < 100`; each of V1, V2, V3, direct Airlock,
  and ERC-7579 is `0 < 10`; detector misses `16 > 0`.
- Pons: quote-eligible `0 < 100`; current-generation profile `0 < 10`.
- hood.fun: quote-eligible `9 < 100`; current-curve profile `9 < 10`;
  detector misses `2 > 0`.

Consequently all 18 readiness booleans across the six launchpads remain
false: no paper-evidence readiness, no canary authorization, and no execution
eligibility.

## Protocol classification

### Bankr/Doppler

Verified facts from the three finalized windows are 16 successful-receipt
Airlock create ground-truth events, no observer claims, no admitted quotes,
and exact miss distribution A/B/C `9/4/3`. Every row is blocked as an Airlock
create calldata shape outside the reviewed standard profiles. The 16 receipt
rows use the reviewed Airlock/event identity, while the existing supported
profile counters for `curve_ticks_v1`, `curve_ticks_v2`, and
`curve_ticks_v3` remain zero.

An exact ABI comparison of the 16 create payloads classifies one new candidate
**curve profile**, named **CurveTicksV4** for research: two curve ranges joined
at `-119200`, with the exact tuple
`[(-229400,-119200,1,990000000000000000),
(-119200,887200,1,10000000000000000)]`. The joint and complete tuple are
identical in Windows A, B, and C. It is not CurveTicksV1, V2, or V3; those
profiles and their fixtures must be preserved unchanged.

Curve version and envelope stratum are orthogonal. CurveTicksV4 appeared in
**two existing strata**: 15 rows used the ERC-7579 path, while Window B
transaction
`0x5fac8d13713912a64bb8ae17563e79d0c162e89a3eb8f5d12b3324d3c9b7558e`
was the one direct-Airlock row and correctly had no `UserOperationEvent`.
This is therefore not one new envelope. The curve evidence does not weaken,
merge, or bypass either stratum's existing identity, wrapper, account,
runtime, beneficiary, or receipt-block pin requirements. “V4” is a bounded
curve taxonomy, not an admitted adapter profile. At base `6e21e69`, V4 remains
unsupported in both strata. No broadening, fallback, quote, plan, or readiness
credit is taken for the 16 misses.

The exact missed transaction identities are reproducible from the
`launchpad_reconciliation_evidence` rows:

```text
Window A (9):
0x05b0ffeb93614eedee2f18b9309fa0dd6aad155cc91f1c200bc32b39561cba55
0x0d7a7e2491ce085bb08b9cd97c8b492b681ece57a9772dc217c96de8ba91ec05
0x9e407b75206c95b3522b35b12d33c7aa4560dd0930032e042538d7ed34b9d716
0x74aae30e530ed4924e2ef6a20066bbad5011c34825bd2e929f98285ce29da3d6
0x7e8166aa043c107c1aeed96d4408906ba93d3a6f3790dcc862d2e1feecf7a537
0x5d8960558ca86480db79dea128351857c021e1aa5f3b3b7018ea5d60fbdfb4a3
0x29794f021ebe8922aeef97721e417352ebdb321b2cd210e779db298d42536953
0x920c52584343fd91c1034869221ba78b162034ce7f0108dace0347bde8cc3992
0x518247e23c9dca90b620483018fa8beae0ce899749c30bbaf245875791cb1da9
Window B (4):
0x5fac8d13713912a64bb8ae17563e79d0c162e89a3eb8f5d12b3324d3c9b7558e
0x9643945aaca673930fbdcad499529510553ae39976475b957da45fde730ea769
0xe57b3cf5738710edc4be8c7c1681c160b0a2fe71697e2c5bf79ea24490044eb1
0x85d67f7304c1418bc771127a18136aed45a05d057685622fd201594e095523bd
Window C (3):
0x85d4ae7a0783bda7be2258762df36b11c78c04bf6d3cbe03504d52da1cd324c4
0x56d9d3c0c8ce10a4d664db4156eeaa2e17a69607680935b64157ff1abde855b6
0x0b6deb541255dee17afcec71ab874aba483dea037aea43240f6229883517705b
```

### hood.fun

The two misses are
`0xd5e26ba81bf2d0c0b016ce21a6b4d2cf06865f3c4824ed1c12af069d1b09960e`
(Window A, L2 `11923936`) and
`0x6676d4fcfc4882e6eeccdb6499056f0c35bee01f3d4144279e443d559dac820b`
(Window B, L2 `11926343`). They are the already-known unsupported
`launchCommunity` wrapper envelope, not a regression in the direct
current-curve profile. Support requires independently verified wrapper and
delegated-implementation identities and runtime hashes, exact ABI/call-shape
recovery, deterministic identity binding, receipt quote semantics, and
negative tests. Observed hashes alone are not authority. The wrapper remains
fail-closed and the two rows remain detector misses; the nine direct eligible
samples do not erase them.

### Pons

Pons produced 59 confirmed observations but zero quote-eligible
current-generation profiles and therefore zero entry/exit plans. The exact
paper-only EIP-7702 self-batch proof is recorded in
`PONS_EIP7702_SELF_BATCH_PROVENANCE_2026-07-16.md`, implemented in
`src/eip7702_self_batch.rs`, and frozen under `.pons_eip7702_self_batch` in
`expected-pins.locked.json` (proof transaction
`0x7a13c94f90ddaa7d35d639f046f30a44d1d9b5fe449550fd0b75e5e65a0fb4c6`,
L2 `11777530`), but that proof/profile did not appear in any of these three
windows. No generic EIP-7702 account, selector, or sibling shape is inferred
from its implementation.

### Bow, LaunchHood V3, and Clanker

Bow had zero activity. LaunchHood V3 supplied nine clean eligible samples and
Clanker supplied four clean eligible samples. Both had zero false positives,
detector or coverage misses, and all mismatch classes zero. They still fail
the minimum aggregate/profile sample gates and remain paper-only.

## Verified facts, inference, and unknowns

**Verified facts.** The digest tuple, three boundary identities, manifest
completion, non-overlap, absence of partial windows, byte-for-byte aggregate
replay, all numeric counters, plan sizing/slippage, and readiness decisions
above are tool-validated frozen-artifact facts. The Bankr miss payloads share
the `-119200` joint and the Hood misses use the unsupported wrapper shape. The
current Pons EIP-7702 proof transaction is absent from these windows.

**Inference/policy classification.** “CurveTicksV4” is a new, deliberately
narrow name for the repeated `-119200` curve shape across the independently
classified ERC-7579 and direct-Airlock strata. Curve version does not identify
or authorize an envelope. The name does not imply that the candidate is safe,
complete, production-supported, or the protocol's own version name. Calling
the Hood rows `launchCommunity` identifies the known wrapper family; it does
not validate the wrapper implementation.

**Unknowns.** This offline audit did not refresh the chain. It does not prove
that any runtime or implementation remains unchanged after the three frozen
snapshot boundaries. The artifacts do not bind themselves to a Git SHA. The
outer campaign runner was not campaign-locked. CurveTicksV4 beneficiary,
complete field invariants, both token orientations, receipt ranges, quote
semantics, and adversarial mutation boundaries are not yet admitted evidence.
The Hood wrapper/implementation pair is not independently pinned. No campaign
window observed the exact Pons current-generation EIP-7702 profile. Bow has no
activity evidence, and all active profiles remain below policy sample floors.

## Next gates

1. For Bankr, independently fixture the exact CurveTicksV4 payload and receipt
   identities in both the ERC-7579 and direct-Airlock strata; retain each
   stratum's complete existing pin boundary; prove the full create envelope,
   beneficiary and both token orientations; add focused positive and one-field
   mutation tests without altering V1-V3; then run new paper windows on a newly
   locked tuple.
2. For Hood, independently verify both `launchCommunity` wrapper and delegated
   implementation pins before adding an exact bounded unwrap/profile; retain
   fail-closed behavior until that review and negative-test matrix pass.
3. For Pons, collect new complete windows that actually contain the exact
   reviewed EIP-7702 current-generation profile. Legacy confirmations do not
   count toward that envelope.
4. Collect enough independent clean observations to satisfy 100 aggregate and
   10-per-profile gates, with every error counter remaining zero. Bow first
   needs any activity evidence.
5. Re-run readiness and evidence aggregation on those new completed manifests.
   A future canary requires a separate explicit authorization; this report
   cannot supply it.

## Reproduction

The following uses the exact campaign binaries that produced the locked tuple.
Both `cmp` commands exit `0`.

```sh
ROOT='/Users/kennethjiang/Documents/hermes-trader 2/hermes-feed/.runtime/paper-campaign-finaltuple-20260716'
BINDIR='/Users/kennethjiang/Documents/hermes-trader 2/hermes-feed/target/release'

"$BINDIR/hermes-launchpad-readiness" \
  --expected-self-keccak256 0x1b125762677439042bac436f37b9b60ce5b0273d641640845e035d5075118f8a \
  --session-dir "$ROOT/windows/window-a" \
  --session-dir "$ROOT/windows/window-b" \
  --session-dir "$ROOT/windows/window-c" | \
cmp - "$ROOT/authoritative-readiness.jsonl"

"$BINDIR/hermes-launchpad-evidence-report" \
  --expected-self-keccak256 0x116f9463f9d1622b386cdf5c6e0dac628c3ec04512f7e104d54117dc15a5a912 \
  --campaign-lock "$ROOT/campaign-lock.json" \
  --snapshot-keccak256 0x0b0ff5ee296e1ef47da19a57e6de04c5e815f72cdec56ba352aaa158f73a1d8b \
  --local-runner-keccak256 0xb065b10aa70b8caaa586298346ac06d11f27871dd6c35e0e6f3249e876356fa3 \
  --readiness-output "$ROOT/authoritative-readiness.jsonl" \
  --expected-pins "$ROOT/expected-pins.locked.json" \
  --readiness-keccak256 0x1b125762677439042bac436f37b9b60ce5b0273d641640845e035d5075118f8a \
  --session-dir "$ROOT/windows/window-a" \
  --session-dir "$ROOT/windows/window-b" \
  --session-dir "$ROOT/windows/window-c" | \
cmp - "$ROOT/launchpad-evidence-report.jsonl"
```

Compact numeric cross-check:

```sh
jq -r '[.launchpad,.observations.confirmed,.observations.eligible,
  .latency_ns.p50,.latency_ns.p95,.latency_ns.p99,
  .errors.false_positives,.errors.detector_misses,
  .errors.identity_mismatches,.errors.direction_mismatches,
  .errors.prediction_mismatches,.errors.quote_mismatches,
  .entry.plan_count,.exit.plan_count,
  (.profiles|map(.profile_envelope+"="+(.observations|tostring))|join(",")),
  .readiness.paper_evidence_ready,.readiness.authorizes_canary,
  .readiness.execution_eligible] | @tsv' \
  "$ROOT/launchpad-evidence-report.jsonl"

for W in a b c; do
  jq -r --arg w "$W" '
    select(.record_type=="launchpad_paper_readiness_window") |
    [$w,.launchpad,.coverage_from_l2_block,.coverage_to_l2_block,
     .quote_eligible_confirmed_observations,.false_positives,
     .detector_misses,.identity_mismatches,.direction_mismatches,
     .prediction_mismatches,.quote_mismatches,
     (.profile_envelope_observations|to_entries|
       map(.key+"="+(.value|tostring))|join(","))] | @tsv' \
    "$ROOT/windows/window-$W/launchpad-paper-finalized.jsonl"
done
```

Miss identities and partial-window check:

```sh
jq -r 'select(.ground_truth_event==true and .observer_claim==false) |
  [.launchpad,.tx_hash,.l2_block_number,.transaction_index,
   .protocol_blocker] | @tsv' \
  "$ROOT"/windows/window-*/reconciliation-evidence.jsonl

find "$ROOT" \( -name '*.partial' -o -name '*partial*' -o -type p -o -type l \) -print
find "$ROOT/windows" -mindepth 1 -maxdepth 1 -type d -print | sort
```

The first `find` prints nothing; the second prints only `window-a`, `window-b`,
and `window-c`. To verify any recorded file hash, run:

```sh
"$BINDIR/hermes-launchpad-evidence-report" --print-file-keccak256 PATH
```
