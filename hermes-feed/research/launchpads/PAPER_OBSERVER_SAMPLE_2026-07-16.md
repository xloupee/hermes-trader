# Paper observer evidence sample: 2026-07-16

This report records a local, read-only five-minute session. No wallet,
keystore, signer, broadcast path, or server was used.

## Session boundary

- Expected authority: `config/launchpad-expected-pins.production.json`
- Fresh observed snapshot: schema 4, 36 pins, L2 block `11619224`, block hash
  `0xd59487deeddb984cf23ee26af67b87463f1d14e1d5cce911e968a8c4498a2139`
- Scored range: L2 blocks `11621789..=11624770`
- Start hash:
  `0xb486e9de75e22f5ad4bb6f7826e0c3938abb47d5820f5ca09884171195dd7581`
- Cutoff hash:
  `0x4f44d7e616e144b3c61f49a83e29a282ca293e88ead5f0fc38f392788afa42ae`
- Complete canonical scan: 2,982 blocks, 48 primary event logs, 48 unique
  protocol keys, two confirmations
- Pipeline: private raw FIFO -> `tee` raw JSONL -> private observer FIFO ->
  paper observer; reconciliation began only after producer close and FIFO drain
- Input/output volume: 3,769 raw frames, 3,770 observer lines, 116
  reconciliation lines

## Original live score

Latency is source receive to observer output. Null latency means there was no
confirmed observation and must not be represented as zero.

| Launchpad | Truth | Confirmed | False positive | Missed (detector / coverage) | Latency p50 / p95 / p99 | Quote result |
|---|---:|---:|---:|---:|---|---|
| Bow | 2 | 2 | 0 | 0 (0 / 0) | 0.265 / 0.357 / 0.357 ms | 0 available, 2 blocked |
| LaunchHood V3 | 2 | 0 | 0 | 2 (2 / 0) | null / null / null | 2 available |
| Clanker | 0 | 0 | 0 | 0 (0 / 0) | null / null / null | no activity |
| Bankr/Doppler | 5 | 1 | 0 | 4 (4 / 0) | 0.323 / 0.323 / 0.323 ms | 0 available, 5 blocked |
| Pons | 39 | 39 | 0 | 0 (0 / 0) | 0.394 / 1.424 / 1.475 ms | 39 legacy, not applicable |
| Hood | 0 | 0 | 0 | 0 (0 / 0) | null / null / null | no activity |

There were 14 out-of-range Pons observations from websocket backlog. They were
classified as out of scope, not false positives. There were also 51 Flap
observer-only discovery records; Flap remains outside the scored production
set until its semantics are complete.

## Prediction and quote findings

- No eligible action prediction mismatched. Legacy Pons had 39/39 action
  matches, but its token and pool predictions were explicitly missing rather
  than guessed.
- LaunchHood's two detector misses were direct canonical calls with nonzero
  value and leader `minTokensOut = 0`. The observer incorrectly treated that
  leader-controlled slippage field as an identity requirement. The corrected
  adapter accepts zero only for observation, explicitly requires `dexId = 0`,
  keeps token/pool prediction unavailable, and remains execution gated.
- Replaying the captured feed through the corrected observer produced two of
  two confirmed LaunchHood observations, zero false positives, zero misses,
  two action matches, and two finalized non-broadcast paper plans. Replay
  latency is intentionally excluded because replay wall time is not live
  observation latency.
- Both Bow launches were correctly observed but the original strict quote
  rejected a payable embedded initial buy. The corrected quoter derives WETH
  input from transaction value and token output from the pool delta, then
  requires independent V3 replay to match both deltas and terminal state. A
  captured-window replay admitted both quotes and finalized two non-broadcast
  paper plans; value, swap, and missing-swap mutations fail closed.
- Four Bankr misses used the exact reviewed ERC-7579 selector, zero mode/value,
  Airlock target, and inner selector, but rotated EIP-7702 accounts with the
  same reviewed designator and Kernel. The fifth was a direct Airlock call.
  The corrected collector discovers only the exact ERC-7579/Airlock structure,
  then independently verifies `ef0100 || implementation`, the designator hash,
  and the delegated Kernel runtime hash at the canonical receipt block before
  granting identity. Direct Airlock envelopes receive the same receipt-block
  EIP-7702 checks.
- Current launches use an exact second reviewed curve profile:
  `(-229600,-119400)` and `(-119400,887200)`. The older proof uses
  `(-229800,-119800)` and `(-119800,887200)`. Both are classified explicitly;
  ranges and partially matching variants remain rejected.
- Captured-feed replay observed all five Bankr launches, reconciled all five,
  admitted five independent quotes, and produced five non-broadcast plans with
  zero misses and zero successful-receipt false positives. Four additional
  in-range observations were byte-identical duplicate submissions two or three
  blocks after a successful launch; all four reverted with zero logs. They are
  reported separately as `reverted_attempts`, not detector false positives.

## LaunchHood independent paper outcomes

Both confirmed LaunchHood receipts independently reconstructed the same fixed
tiny policy outcome after the embedded launch swap:

- Entry size: `0.001 WETH`
- Expected entry output: `729729.244852920426979991` tokens
- Entry minimum at 1% slippage: `722431.952404391222710191` tokens
- Immediate full-position exit: `0.000980107152180750 WETH`
- Exit minimum at 1% slippage: `0.000970306080658942 WETH`
- Simulated round-trip return: `9801 bps`
- Execution eligible: false
- Broadcast: false

These are paper simulations from receipt-end state, not executable or live
quotes. The token restriction and runtime checks remain unsatisfied.

## Bow independent paper outcomes

The two payable Bow receipts produced distinct receipt-end quotes under the
same fixed tiny policy:

| Transaction | Entry output | Entry minimum | Full exit | Exit minimum | Return |
|---|---:|---:|---:|---:|---:|
| `0x6ee43f...8458` | `616344.855547336185380396` | `610181.406991862823526592` | `0.000980106129042453 WETH` | `0.000970305067752028 WETH` | `9801 bps` |
| `0xf84259...ab6b` | `618664.675371529997585409` | `612478.028617814697609554` | `0.000980106140562431 WETH` | `0.000970305079156806 WETH` | `9801 bps` |

Each entry size is `0.001 WETH`, each minimum applies 1% slippage, and each
plan remains `execution_eligible: false` and `broadcast: false`.

## Bankr/Doppler independent paper outcomes

All five current curve-profile receipts independently reconstructed the same
receipt-end state and fixed tiny policy outcome:

- Entry size: `0.001 WETH`
- Expected entry output: `922744.419124464731197397` tokens
- Entry minimum at 1% slippage: `913516.974933220083885423` tokens
- Immediate full-position exit: `0.000009736765175891 WETH`
- Exit minimum at 1% slippage: `0.000009639397524132 WETH`
- Simulated round-trip return: `97 bps`
- Envelope coverage: four ERC-7579/EIP-7702, one direct Airlock/EIP-7702
- Execution eligible: false
- Broadcast: false

The low immediate-exit result reflects the independently modeled launch-time
Rehype fee schedule. It is evidence against promotion, not an executable quote.

## Promotion decision

No launchpad is ready for a canary from this sample. Bow, LaunchHood, and Bankr
need a fresh post-fix live window with valid latency and a larger confirmed
sample. Bankr's `97 bps` immediate round trip is also a direct promotion
blocker. Pons activity was legacy discovery-only. Clanker and Hood had no
events in this window. Any canary remains separately approval-gated and out of
scope for local work.
